use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition};

pub const TRAY_ID: &str = "s2akit-tray";

/// 托盘菜单逻辑宽度（CSS px），与 tauri.conf 中 tray-menu 的 width 保持一致。
/// 宽度取固定值：hidden 窗口的 scale 可能失准，用 outer_size 反推宽度会逐次漂移
const MENU_WIDTH_LOGICAL: f64 = 240.0;

/// 按账号数估算菜单逻辑高度：顶部单行 ~28 + 账号行 32/条（紧凑布局）
/// + 底部操作 4 项 32/条 + 分隔线与余量
fn estimate_menu_height(n: usize) -> f64 {
    (28.0 + n as f64 * 32.0 + 4.0 * 32.0 + 6.0).clamp(220.0, 640.0)
}

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
    // 菜单已可见时再右键：按原生行为直接收起。
    // 否则会与失焦自动隐藏打架（先 show 后被迟到的 Focused(false) 藏掉，
    // 或反复横跳），表现为菜单不跟手/闪烁
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    // 记录右键位置为菜单锚点：初始估算定位与 fit_menu 的重定位都以它为准，
    // 保证菜单底角始终贴住点击位置（与其他托盘图标的原生菜单一致）
    // poison 时取内部值继续，避免托盘线程 panic
    *app.state::<crate::state::AppState>()
        .menu_anchor
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some((cursor.x, cursor.y));
    // 先按账号数粗估高度立即弹出，前端渲染完成后会经 fit_menu 用真实内容高度修正
    let n = app
        .state::<crate::state::AppState>()
        .accounts
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .len();
    let logical_h = estimate_menu_height(n);
    place_menu(app, &w, logical_h);
    show_menu(app);
    // show() 之后再定位一次：show 可能重设窗口状态，此时窗口可见、
    // scale 与尺寸均已权威，锚点定位精确生效（与 fit_menu 的后续修正互补）
    place_menu(app, &w, logical_h);
}

/// 前端回报真实内容高度后重设窗口尺寸并按锚点重新定位（仅菜单可见时生效）
pub fn fit_menu_window(app: &AppHandle, height: f64) {
    let Some(w) = app.get_webview_window("tray-menu") else {
        return;
    };
    // 菜单已隐藏后到达的迟到回报直接忽略（锚点可能已过期）
    if !w.is_visible().unwrap_or(false) {
        return;
    }
    let scale = w.scale_factor().unwrap_or(1.0);
    // 内容超出工作区高度时封顶，超出部分由菜单内账号列表滚动
    let cap = app
        .state::<crate::state::AppState>()
        .menu_anchor
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .and_then(|(x, y)| work_area_at(app, x, y))
        .map(|(_, top, _, bottom)| (bottom - top) / scale)
        .unwrap_or(640.0);
    place_menu(app, &w, height.clamp(120.0, cap));
}

/// 设置菜单窗口高度，并把菜单底角贴到锚点光标处（允许盖住任务栏上沿，与原生行为一致）
fn place_menu(app: &AppHandle, w: &tauri::WebviewWindow, logical_h: f64) {
    let scale = w.scale_factor().unwrap_or(1.0);
    let _ = w.set_size(LogicalSize::new(MENU_WIDTH_LOGICAL, logical_h));
    let menu_w = MENU_WIDTH_LOGICAL * scale;
    let menu_h = logical_h * scale;
    let Some(anchor) = *app
        .state::<crate::state::AppState>()
        .menu_anchor
        .lock()
        .unwrap_or_else(|e| e.into_inner())
    else {
        return;
    };
    // 用显示器完整矩形定位（而非工作区）：原生 TPM_BOTTOMALIGN 把菜单底边贴到
    // 光标处、允许盖住任务栏；若按工作区钳位，菜单会永远悬在光标上方 ~24px
    let Some(mon) = monitor_rect_at(app, anchor.0, anchor.1) else {
        return;
    };
    let (x, y) = menu_origin(anchor, (menu_w, menu_h), mon);
    let _ = w.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

/// 原生托盘菜单（TrackPopupMenu, TPM_BOTTOMALIGN|TPM_LEFTALIGN）式定位：
/// 菜单左缘对准光标、底边精确贴到光标处向上弹出（允许盖住任务栏上沿）；
/// 右侧放不下时水平翻转（光标改贴菜单右下角），最后整体钳入显示器。
/// 注意矩形参数是显示器完整矩形（含任务栏），不是工作区——按工作区钳位
/// 会把菜单顶到任务栏上沿，底部与光标之间永远差出任务栏一半高度。
/// 返回菜单窗口左上角坐标。
fn menu_origin(
    (cx, cy): (f64, f64),
    (menu_w, menu_h): (f64, f64),
    (left, top, right, bottom): (f64, f64, f64, f64),
) -> (f64, f64) {
    let x = if cx + menu_w <= right { cx } else { cx - menu_w };
    let x = x.clamp(left, (right - menu_w).max(left));
    // 底边贴光标；菜单比光标上方空间还高时顶边贴显示器上沿（超出部分继续盖住任务栏）
    let y = (cy - menu_h).max(top).min((bottom - menu_h).max(top));
    (x, y)
}

/// 光标所在的显示器；找不到时退回主显示器
fn monitor_at(app: &AppHandle, x: f64, y: f64) -> Option<tauri::Monitor> {
    if let Ok(mons) = app.available_monitors() {
        for m in mons {
            let p = m.position();
            let s = m.size();
            if x >= p.x as f64
                && x < (p.x + s.width as i32) as f64
                && y >= p.y as f64
                && y < (p.y + s.height as i32) as f64
            {
                return Some(m);
            }
        }
    }
    app.primary_monitor().ok().flatten()
}

/// 光标所在显示器的完整矩形（left, top, right, bottom，物理像素，含任务栏区域）；
/// 找不到时退回主显示器
fn monitor_rect_at(app: &AppHandle, x: f64, y: f64) -> Option<(f64, f64, f64, f64)> {
    monitor_at(app, x, y).map(|m| {
        let p = m.position();
        let s = m.size();
        (
            p.x as f64,
            p.y as f64,
            (p.x + s.width as i32) as f64,
            (p.y + s.height as i32) as f64,
        )
    })
}

/// 光标所在显示器的工作区（left, top, right, bottom，物理像素，已排除任务栏）；
/// 找不到时退回主显示器
fn work_area_at(app: &AppHandle, x: f64, y: f64) -> Option<(f64, f64, f64, f64)> {
    monitor_at(app, x, y).map(|m| {
        let wa = m.work_area();
        (
            wa.position.x as f64,
            wa.position.y as f64,
            (wa.position.x + wa.size.width as i32) as f64,
            (wa.position.y + wa.size.height as i32) as f64,
        )
    })
}

fn show_menu(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("tray-menu") {
        // 上次显示后前端未回 pong（如 dev 服务器重启后页面停在错误页）则先重进页面再显示，
        // 菜单在服务器恢复后即可自愈；服务器仍在则重复导航无副作用。
        // 用宿主级 navigate 而非 eval：页面停在 WebView 错误页（连接被拒等）时 eval 不保证执行
        let alive = app
            .state::<crate::state::AppState>()
            .menu_alive
            .swap(false, std::sync::atomic::Ordering::Relaxed);
        if !alive {
            match w.url().ok().filter(|u| u.scheme() != "about") {
                Some(u) => {
                    let _ = w.navigate(u);
                }
                None => {
                    let _ = w.eval("location.reload()");
                }
            }
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

#[cfg(test)]
mod tests {
    use super::{estimate_menu_height, menu_origin};

    #[test]
    fn bottom_taskbar_overlaps_toward_cursor() {
        // 1920x1080 底部任务栏（任务栏 1032~1080），在图标中心 y=1056 右键：
        // 菜单向右展开、底边精确贴到光标（756+300=1056），盖住任务栏上沿 24px，
        // 与原生 TPM_BOTTOMALIGN|TPM_LEFTALIGN 一致
        let (x, y) = menu_origin((1550.0, 1056.0), (360.0, 300.0), (0.0, 0.0, 1920.0, 1080.0));
        assert_eq!((x, y), (1550.0, 756.0));
    }

    #[test]
    fn near_right_edge_flips_left() {
        // 图标贴近屏幕右缘：菜单水平翻转，点击位置即菜单右下角
        let (x, y) = menu_origin((1900.0, 1056.0), (360.0, 300.0), (0.0, 0.0, 1920.0, 1080.0));
        assert_eq!((x, y), (1540.0, 756.0)); // 右缘 1540+360=1900 对准光标
    }

    #[test]
    fn top_taskbar_covers_downward() {
        // 顶部任务栏（0~48），在图标中心 y=24 右键：菜单顶边贴显示器上沿，
        // 底边落到光标下方，盖住任务栏区域（原生行为）
        let (x, y) = menu_origin((1552.0, 24.0), (360.0, 300.0), (0.0, 0.0, 1920.0, 1080.0));
        assert_eq!((x, y), (1552.0, 0.0));
    }

    #[test]
    fn taller_than_monitor_pins_top() {
        // 菜单比显示器还高（实际被 fit 高度上限拦住，此处只验证不 panic、有确定输出）：
        // 顶边贴显示器上沿
        let (x, y) = menu_origin((1552.0, 680.0), (360.0, 900.0), (0.0, 0.0, 1920.0, 700.0));
        assert_eq!((x, y), (1552.0, 0.0));
    }

    #[test]
    fn estimate_height_matches_row_layout() {
        // 0 账号：28 + 0 + 128 + 6 = 162，触底钳到 220
        assert_eq!(estimate_menu_height(0), 220.0);
        // 10 账号：28 + 320 + 128 + 6 = 482
        assert_eq!(estimate_menu_height(10), 482.0);
        // 账号再多封顶 640，超出部分由菜单内列表滚动
        assert_eq!(estimate_menu_height(100), 640.0);
    }
}
