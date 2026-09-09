mod commands;
mod config;
mod error;
mod state;
mod sub2api;
mod tray;
#[cfg(windows)]
mod win_toast;

use config::AppConfig;
use state::{AppState, AuthEntry};
use std::sync::atomic::AtomicBool;
use std::sync::RwLock;
use std::time::Duration;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let handle = app.handle().clone();

            let config_path = handle
                .path()
                .app_config_dir()
                .map_err(|e| format!("无法定位配置目录: {e}"))?
                .join("config.json");
            let cfg = AppConfig::load(&config_path);
            let http = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

            app.manage(AppState {
                http,
                config: RwLock::new(cfg),
                config_path,
                auth: RwLock::<Option<AuthEntry>>::new(None),
                group_name: RwLock::new("未选择分组".into()),
                accounts: RwLock::new(Vec::new()),
                groups: RwLock::new(Vec::new()),
                last_results: RwLock::new(std::collections::HashMap::new()),
                testing: AtomicBool::new(false),
                usage_cache: RwLock::new(None),
                menu_alive: AtomicBool::new(true),
                menu_anchor: std::sync::Mutex::new(None),
                submenu_row_top: std::sync::Mutex::new(None),
            });

            tray::setup_tray(&handle)?;

            // 菜单窗口 DWM 圆角：CSS 只切网页，窗口本身也要裁，
            // 否则亚克力铺满矩形、四角露出方形底（Win10 忽略，保持方形）
            #[cfg(windows)]
            {
                if let Some(menu) = app.get_webview_window("tray-menu") {
                    tray::round_menu_corners(&menu);
                }
                if let Some(sub) = app.get_webview_window("tray-submenu") {
                    tray::round_menu_corners(&sub);
                }
            }

            // 注册通知 AUMID(开发/便携模式下让 Toast 显示应用图标而非 PowerShell)
            #[cfg(windows)]
            win_toast::ensure_aumid_shortcut(&handle);

            // 关闭窗口时隐藏到托盘，真正退出走托盘菜单
            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            // 托盘菜单窗口：失焦即隐藏（模拟原生菜单行为）。
            // 失焦后延迟 150ms 二次确认：子菜单窗口弹出瞬间（SW_SHOW）会短暂抢走焦点，
            // show_submenu 会立刻归还，二次确认时已恢复聚焦则不收；真正的点消仍会收起。
            // 记录“获得过焦点”状态：显示瞬间可能先收到一次 Focused(false)，需忽略
            if let Some(menu) = app.get_webview_window("tray-menu") {
                let m = menu.clone();
                let app_h = handle.clone();
                let had_focus = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let hf = had_focus.clone();
                menu.on_window_event(move |event| match event {
                    tauri::WindowEvent::Focused(true) => {
                        hf.store(true, std::sync::atomic::Ordering::Relaxed);
                        // tao show 后可能重设扩展样式，重应用菜单透明度（幂等）
                        crate::commands::apply_menu_opacity(&app_h);
                    }
                    tauri::WindowEvent::Focused(false) => {
                        if hf.swap(false, std::sync::atomic::Ordering::Relaxed) {
                            let app_c = app_h.clone();
                            let m_c = m.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(150));
                                let focused = app_c
                                    .get_webview_window("tray-menu")
                                    .and_then(|w| w.is_focused().ok())
                                    .unwrap_or(false);
                                if !focused {
                                    let _ = m_c.hide();
                                    crate::tray::hide_submenu_window(&app_c);
                                }
                            });
                        }
                    }
                    tauri::WindowEvent::CloseRequested { .. } => {
                        let _ = m.hide();
                    }
                    _ => {}
                });
            }

            // 后台静默初始化：自动登录 + 加载分组/账号 + 重建托盘
            tauri::async_runtime::spawn(async move {
                commands::startup_init(handle).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::login,
            commands::login_2fa,
            commands::logout,
            commands::get_auth,
            commands::get_compliance,
            commands::accept_compliance,
            commands::list_groups,
            commands::list_accounts,
            commands::select_group,
            commands::get_account_models,
            commands::set_schedulable,
            commands::test_account,
            commands::tray_test_account,
            commands::get_last_results,
            commands::test_all,
            commands::get_key_usage_today,
            commands::set_menu_opacity,
            commands::menu_pong,
            commands::fit_menu,
            commands::show_submenu,
            commands::show_groups_submenu,
            commands::hide_submenu,
            commands::fit_submenu,
            commands::open_main_window,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running s2akit");
}
