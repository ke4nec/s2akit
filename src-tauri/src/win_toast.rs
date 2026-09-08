//! Windows Toast 直发 + AppUserModelID(AUMID)注册。
//!
//! tauri-plugin-notification 在 exe 位于 target\debug|release(开发/便携)时
//! 不设置 System.AppUserModel.ID,底层 notify-rust 便回退到 PowerShell 的
//! AUMID,通知因此显示 PowerShell 图标与应用名。Toast 的图标和名称取自
//! AUMID 对应的开始菜单快捷方式,所以这里:
//! 1. 非安装模式启动时,在开始菜单创建带 AppUserModel.ID 的快捷方式;
//! 2. 发通知时显式指定 AUMID(安装模式直接用 identifier,已由安装器注册)。

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// 便携/开发模式的 AUMID 与安装版 identifier 区分开,
/// 避免抢占安装器为正式版创建的同名快捷方式
fn portable_aumid(identifier: &str) -> String {
    format!("{identifier}.portable")
}

/// exe 是否位于安装目录。Tauri 安装器:MSI / NSIS perMachine 落在 Program Files,
/// NSIS currentUser(默认)落在 %LOCALAPPDATA%\<productName>;其快捷方式已由
/// 安装器注册 identifier,无需再注册。%LOCALAPPDATA%\Programs 前缀是部分
/// 安装器的约定位置,一并覆盖
fn in_install_dir(exe: &Path, product_name: &str) -> bool {
    let Some(dir) = exe.parent() else {
        return true;
    };
    let dir_s = dir.as_os_str().to_string_lossy().to_lowercase();
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(pf) = std::env::var_os(var) {
            let pf_s = PathBuf::from(pf).as_os_str().to_string_lossy().to_lowercase();
            if dir_s == pf_s || dir_s.starts_with(&format!("{pf_s}\\")) {
                return true;
            }
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local_s = PathBuf::from(local).as_os_str().to_string_lossy().to_lowercase();
        let app_s = format!(r"{}\{}", local_s, product_name.to_lowercase());
        let programs_s = format!(r"{}\programs", local_s);
        if dir_s == app_s
            || dir_s == programs_s
            || dir_s.starts_with(&format!(r"{}\programs\", local_s))
        {
            return true;
        }
    }
    false
}

/// 便携模式快捷方式是否注册成功;失败时通知退回插件原行为(PowerShell 图标,至少能显示)
static PORTABLE_AUMID_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 启动时调用:为开发/便携运行的 exe 注册开始菜单快捷方式
pub fn ensure_aumid_shortcut(app: &AppHandle) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let identifier = app.config().identifier.clone();
    let product = app.package_info().name.clone();
    if in_install_dir(&exe, &product) {
        return;
    }
    let link_name = format!("{} (portable)", app.package_info().name);
    std::thread::spawn(move || {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return;
        };
        let dir = PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let link = dir.join(format!("{link_name}.lnk"));
        match write_shortcut(&exe, &link, &portable_aumid(&identifier)) {
            Ok(()) => PORTABLE_AUMID_READY.store(true, std::sync::atomic::Ordering::Relaxed),
            Err(e) => eprintln!("注册通知 AUMID 快捷方式失败: {e}"),
        }
    });
}

/// 发送 Toast,显式指定 AUMID 以显示应用自己的图标
pub fn notify(app: &AppHandle, title: &str, body: &str) {
    let identifier = app.config().identifier.clone();
    let product = app.package_info().name.clone();
    let mut n = notify_rust::Notification::new();
    n.summary(title).body(body);
    // current_exe 失败时不指定 app_id,回退到 notify-rust 默认行为(至少能显示)
    if let Ok(exe) = std::env::current_exe() {
        if in_install_dir(&exe, &product) {
            // 安装版:AUMID 已由安装器注册
            n.app_id(&identifier);
        } else if PORTABLE_AUMID_READY.load(std::sync::atomic::Ordering::Relaxed) {
            n.app_id(&portable_aumid(&identifier));
        }
        // 未就绪时不指定 app_id,notify-rust 回退 PowerShell,与插件行为一致
    }
    // show() 是阻塞的 WinRT/COM 调用,用独立线程发送,避免占用 Tauri 异步运行时
    std::thread::spawn(move || {
        let _ = n.show();
    });
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

// {00021401-0000-0000-C000-000000000046},ShellLink coclass
const CLSID_SHELL_LINK: windows::core::GUID =
    windows::core::GUID::from_u128(0x00021401_0000_0000_c000_000000000046);

// System.AppUserModel.ID 的 PROPERTYKEY:{9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3} pid=5
const PKEY_APP_USER_MODEL_ID: windows::Win32::Foundation::PROPERTYKEY =
    windows::Win32::Foundation::PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 5,
    };

/// 用 COM 创建快捷方式并写入 System.AppUserModel.ID。
/// 调用方保证当前线程可安全初始化 STA。
fn write_shortcut(exe: &Path, link: &Path, aumid: &str) -> windows::core::Result<()> {
    use windows::core::{Interface, PCWSTR, PWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, IPersistFile,
    };
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::IShellLinkW;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() {
            return Err(hr.into());
        }

        let result = (|| -> windows::core::Result<()> {
            let shell_link: IShellLinkW =
                CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER)?;

            let exe_w = wide(exe);
            let exe_p = PCWSTR(exe_w.as_ptr());
            let dir_w = wide(&exe.parent().unwrap_or(Path::new(".")));
            shell_link.SetPath(exe_p)?;
            shell_link.SetWorkingDirectory(PCWSTR(dir_w.as_ptr()))?;
            shell_link.SetIconLocation(exe_p, 0)?;

            // 先写属性再落盘:IPersistFile::Save 才会把 AppUserModel.ID 带进 .lnk。
            // VT_LPWSTR 的 PROPVARIANT 在 Drop 时被 windows crate 用 PropVariantClear
            // （即 CoTaskMemFree）释放，字符串必须来自 CoTaskMemAlloc；
            // 直接挂 Vec 缓冲区指针会被错误释放，导致堆损坏（0xc0000374）启动崩溃
            let store: IPropertyStore = shell_link.cast()?;
            let aumid_w: Vec<u16> = aumid.encode_utf16().chain([0]).collect();
            let aumid_buf = CoTaskMemAlloc(aumid_w.len() * std::mem::size_of::<u16>());
            if aumid_buf.is_null() {
                return Err(windows::core::Error::from_win32());
            }
            std::ptr::copy_nonoverlapping(aumid_w.as_ptr(), aumid_buf as *mut u16, aumid_w.len());
            let mut pv: PROPVARIANT = std::mem::zeroed();
            (*pv.Anonymous.Anonymous).vt = VT_LPWSTR;
            (*pv.Anonymous.Anonymous).Anonymous.pwszVal = PWSTR(aumid_buf as *mut u16);
            store.SetValue(&PKEY_APP_USER_MODEL_ID, &pv)?;
            store.Commit()?;

            let persist: IPersistFile = shell_link.cast()?;
            let mut link_w = wide(link);
            persist.Save(PWSTR(link_w.as_mut_ptr()), true)?;
            Ok(())
        })();
        CoUninitialize();
        result
    }
}
