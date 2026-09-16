//! Linux 原生托盘菜单（GNOME/KDE 等无自绘托盘窗口的环境）
//!
//! libappindicator 向 StatusNotifierWatcher 注册的硬性前提是挂了 GTK 菜单，
//! 且 tray-icon 的 Linux 后端不派发点击事件——Windows 侧的右键自绘菜单窗口
//! 在 Linux 上既触发不了、也导致托盘图标静默不注册。因此 Linux 下改用
//! tauri/muda 原生菜单承载同等功能（分组/账号/Key/刷新/退出）。

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

type Item = Box<dyn IsMenuItem<tauri::Wry>>;

fn refs(items: &[Item]) -> Vec<&dyn IsMenuItem<tauri::Wry>> {
    items.iter().map(|i| i.as_ref()).collect()
}

/// 注册全局菜单事件分发（setup 时调用一次）
pub fn init(app: &AppHandle) {
    app.on_menu_event(|handle, event| {
        let action = event.id().as_ref().to_string();
        handle_menu_action(handle, &action);
    });
}

/// 账号/Key/分组数据变化后重建菜单（refresh_accounts_task / list_keys 调用）。
/// muda 菜单只能在主线程操作，异步上下文调用时切回主线程
pub fn rebuild(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = handle.tray_by_id(crate::tray::TRAY_ID) {
            if let Ok(menu) = build_menu(&handle) {
                let _ = tray.set_menu(Some(menu));
            }
        }
    });
}

fn disabled_item(app: &AppHandle, text: &str) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(app, format!("disabled-{text}"), text, false, None::<&str>)
}

pub(crate) fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let state = app.state::<AppState>();
    let cfg = state.config_snapshot();
    let group_name = state.group_name.read().unwrap().clone();
    let accounts = state.accounts.read().unwrap().clone();
    let groups = state.groups.read().unwrap().clone();
    let keys = state.keys.read().unwrap().clone();

    let open = MenuItem::with_id(app, "open", "打开主窗口", true, None::<&str>)?;
    let mut items: Vec<Item> = vec![Box::new(open)];

    // 分组：单选语义用 CheckMenuItem 表达，当前组打勾
    let group_title = if group_name.is_empty() {
        "分组".to_string()
    } else {
        format!("分组（{group_name}）")
    };
    let mut group_items: Vec<Item> = vec![Box::new(
        CheckMenuItem::with_id(app, "grp:all", "全部账号", cfg.group_id.is_none(), true, None::<&str>)?,
    )];
    for g in &groups {
        group_items.push(Box::new(CheckMenuItem::with_id(
            app,
            format!("grp:{}", g.id),
            &g.name,
            cfg.group_id == Some(g.id),
            true,
            None::<&str>,
        )?));
    }
    if groups.is_empty() {
        group_items.push(Box::new(disabled_item(app, "暂无分组（未登录）")?));
    }
    items.push(Box::new(Submenu::with_id_and_items(
        app,
        "submenu-groups",
        group_title,
        true,
        &refs(&group_items),
    )?));

    // 账号：每账号一个二级菜单（测试 / 调度开关），行首圆点对齐自绘菜单
    let mut account_items: Vec<Item> = Vec::new();
    for a in &accounts {
        let mut label = format!("{} {}", if a.schedulable { "●" } else { "○" }, a.name);
        if a.rate_limited {
            label.push_str("（限流）");
        }
        if a.status == "error" {
            label.push_str(" !");
        }
        let sub_items: Vec<Item> = vec![
            Box::new(MenuItem::with_id(
                app,
                format!("acctest:{}", a.id),
                "测试该账号",
                true,
                None::<&str>,
            )?),
            Box::new(CheckMenuItem::with_id(
                app,
                format!("accsw:{}", a.id),
                "参与调度",
                a.schedulable,
                true,
                None::<&str>,
            )?),
        ];
        account_items.push(Box::new(Submenu::with_id_and_items(
            app,
            format!("submenu-acc-{}", a.id),
            label,
            true,
            &refs(&sub_items),
        )?));
    }
    if account_items.is_empty() {
        account_items.push(Box::new(disabled_item(app, "暂无账号（未登录？）")?));
    }
    items.push(Box::new(Submenu::with_id_and_items(
        app,
        "submenu-accounts",
        "账号",
        true,
        &refs(&account_items),
    )?));

    // API Key：每 key 一个二级菜单（测试 / 启停）
    let mut key_items: Vec<Item> = Vec::new();
    for k in &keys {
        let enabled = k.status == "active";
        let sub_items: Vec<Item> = vec![
            Box::new(MenuItem::with_id(
                app,
                format!("keytest:{}", k.id),
                "测试该 Key",
                true,
                None::<&str>,
            )?),
            Box::new(CheckMenuItem::with_id(
                app,
                format!("keysw:{}", k.id),
                "启用",
                enabled,
                true,
                None::<&str>,
            )?),
        ];
        key_items.push(Box::new(Submenu::with_id_and_items(
            app,
            format!("submenu-key-{}", k.id),
            &k.name,
            true,
            &refs(&sub_items),
        )?));
    }
    if key_items.is_empty() {
        key_items.push(Box::new(disabled_item(app, "暂无 API Key")?));
    }
    items.push(Box::new(Submenu::with_id_and_items(
        app,
        "submenu-keys",
        "API Key",
        true,
        &refs(&key_items),
    )?));

    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(MenuItem::with_id(app, "refresh", "刷新", true, None::<&str>)?));
    items.push(Box::new(MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?));

    Menu::with_items(app, &refs(&items))
}

/// 菜单动作统一按 id 前缀分发；异步命令走 async_runtime，不阻塞菜单回调
fn handle_menu_action(app: &AppHandle, action: &str) {
    let handle = app.clone();
    match action {
        "open" => crate::tray::show_main_window(&handle),
        "quit" => handle.exit(0),
        "refresh" => {
            tauri::async_runtime::spawn(async move {
                let _ = crate::commands::refresh_accounts_task(&handle).await;
                let _ = crate::commands::list_keys(handle.clone()).await;
            });
        }
        "grp:all" => pick_group(handle, None),
        gid if gid.starts_with("grp:") => {
            let Ok(id) = gid["grp:".len()..].parse::<i64>() else { return };
            pick_group(handle, Some(id));
        }
        act if act.starts_with("acctest:") => {
            let Ok(id) = act["acctest:".len()..].parse::<i64>() else { return };
            tauri::async_runtime::spawn(async move {
                let _ = crate::commands::test_account_task(handle, id, None).await;
            });
        }
        act if act.starts_with("accsw:") => {
            let Ok(id) = act["accsw:".len()..].parse::<i64>() else { return };
            let want = !crate::commands::account_schedulable(&handle, id);
            tauri::async_runtime::spawn(async move {
                // 失败时重建菜单回滚勾选视觉态（成功路径 refresh_accounts_task 已重建）
                if crate::commands::set_schedulable(handle.clone(), id, want)
                    .await
                    .is_err()
                {
                    crate::tray::refresh_native_menu(&handle);
                }
            });
        }
        act if act.starts_with("keysw:") => {
            let Ok(id) = act["keysw:".len()..].parse::<i64>() else { return };
            let want = !crate::commands::key_enabled(&handle, id);
            tauri::async_runtime::spawn(async move {
                if crate::commands::set_key_enabled(handle.clone(), id, want)
                    .await
                    .is_err()
                {
                    crate::tray::refresh_native_menu(&handle);
                }
            });
        }
        act if act.starts_with("keytest:") => {
            let Ok(id) = act["keytest:".len()..].parse::<i64>() else { return };
            let key = handle
                .state::<AppState>()
                .keys
                .read()
                .unwrap()
                .iter()
                .find(|k| k.id == id)
                .cloned();
            let Some(k) = key else { return };
            tauri::async_runtime::spawn(async move {
                let _ = crate::commands::test_key_task(&handle, k.id, &k.name, &k.key, None).await;
            });
        }
        _ => {}
    }
}

/// 切换目标分组：group_id=None 表示“全部账号”（select_group 命令只收 i64）
fn pick_group(app: AppHandle, group_id: Option<i64>) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let mut cfg = state.config_snapshot();
        cfg.group_id = group_id;
        if let Err(e) = state.save_config(&cfg) {
            eprintln!("save config failed: {e}");
            crate::tray::refresh_native_menu(&app);
            return;
        }
        drop(state);
        // 失败时重建菜单回滚勾选视觉态（成功路径 refresh_accounts_task 已重建）
        if crate::commands::refresh_accounts_task(&app).await.is_err() {
            crate::tray::refresh_native_menu(&app);
        }
    });
}
