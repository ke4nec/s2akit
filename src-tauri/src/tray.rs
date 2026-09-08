use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition};

pub const TRAY_ID: &str = "s2akit-tray";

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(
            app.default_window_icon()
                .expect("window icon missing")
                .clone(),
        )
        .tooltip("s2akit - sub2api 账号工具")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                let app = tray.app_handle();
                match button {
                    MouseButton::Left => toggle_main_window(app),
                    MouseButton::Right => show_menu_window(app, position),
                    _ => {}
                }
            }
        })
        .build(app)?;
    Ok(())
}

/// 在托盘图标旁弹出半透明菜单窗口
pub fn show_menu_window(app: &AppHandle, cursor: PhysicalPosition<f64>) {
    let Some(w) = app.get_webview_window("tray-menu") else {
        return;
    };
    // 按当前账号数估算高度，让窗口贴合内容（宽度沿用窗口配置尺寸）
    let n = app
        .state::<crate::state::AppState>()
        .accounts
        .read()
        .unwrap()
        .len();
    let scale = w.scale_factor().unwrap_or(1.0);
    let menu_w = w
        .outer_size()
        .map(|s| s.width as f64)
        .unwrap_or(240.0 * scale);
    // 顶部单行（分组+用量）~28 + 账号行 32/条（紧凑布局）+ 底部操作 4 项 32/条 + 分隔线与余量
    let logical_h = (28.0 + n as f64 * 32.0 + 4.0 * 32.0 + 6.0).clamp(220.0, 640.0);
    let _ = w.set_size(LogicalSize::new(menu_w / scale, logical_h));
    let menu_h = logical_h * scale;

    // 让菜单出现在光标左上侧，并钳位在工作区（排除任务栏）内，避免被任务栏遮挡
    let mut x = cursor.x - menu_w;
    let mut y = cursor.y - menu_h;
    if let Ok(Some(mon)) = w.current_monitor() {
        let wa = mon.work_area();
        let right = wa.position.x as f64 + wa.size.width as f64;
        let bottom = wa.position.y as f64 + wa.size.height as f64;
        x = x.max(wa.position.x as f64 + 8.0).min(right - menu_w - 8.0);
        y = y
            .max(wa.position.y as f64 + 8.0)
            .min(bottom - menu_h - 8.0);
    }
    let _ = w.set_position(PhysicalPosition::new(x as i32, y as i32));
    show_menu(app);
}

fn show_menu(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("tray-menu") {
        // 上次显示后前端未回 pong（如 dev 服务器重启后页面停在错误页）则先重载再显示，
        // 菜单在服务器恢复后即可自愈；服务器仍在则重载无副作用
        let alive = app
            .state::<crate::state::AppState>()
            .menu_alive
            .swap(false, std::sync::atomic::Ordering::Relaxed);
        if !alive {
            let _ = w.eval("location.reload()");
        }
        let _ = w.set_always_on_top(true);
        let _ = w.show();
        let _ = w.set_focus();
        // 注意顺序：tao 的 show() 会重设窗口扩展样式、抹掉 WS_EX_LAYERED，
        // 因此 alpha 必须在 show 之后应用
        crate::commands::apply_menu_opacity(app);
        let _ = app.emit_to("tray-menu", "tray-menu-shown", ());
    }
}

pub fn toggle_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_main_window(app);
        }
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
