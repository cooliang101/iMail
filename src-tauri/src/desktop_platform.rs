use std::path::{Path, PathBuf};

const WEBVIEW2_DOWNLOAD_URL: &str =
    "https://developer.microsoft.com/microsoft-edge/webview2/#download-section";

/// Checks the system WebView2 runtime before Tauri attempts to create any
/// windows. The guidance must be native because there is no WebView available
/// to render an in-app error in this failure mode.
pub(crate) fn ensure_webview2_runtime() -> bool {
    #[cfg(windows)]
    {
        match tauri::webview_version() {
            Ok(version) => {
                log::info!(target: "desktop", "[webview2.ready] version={version}");
                true
            }
            Err(error) => {
                log::error!(target: "desktop", "[webview2.missing] {error}");
                show_webview2_download_guidance();
                false
            }
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[cfg(windows)]
fn show_webview2_download_guidance() {
    use windows::{
        core::{w, HSTRING},
        Win32::UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{
                MessageBoxW, IDYES, MB_DEFBUTTON1, MB_ICONERROR, MB_SETFOREGROUND, MB_YESNO,
                SW_SHOWNORMAL,
            },
        },
    };

    let message = HSTRING::from(format!(
        "iMail 需要 Microsoft Edge WebView2 Runtime 才能显示界面，但当前用户未检测到该组件。\n\n请选择“是”打开微软官方下载页，下载并运行“Evergreen Bootstrapper”。安装完成后，请重新启动 iMail。\n\n官方下载地址：\n{WEBVIEW2_DOWNLOAD_URL}\n\n是否现在打开下载页面？"
    ));

    let choice = unsafe {
        MessageBoxW(
            None,
            &message,
            w!("iMail 需要 WebView2"),
            MB_YESNO | MB_ICONERROR | MB_DEFBUTTON1 | MB_SETFOREGROUND,
        )
    };

    if choice == IDYES {
        let url = HSTRING::from(WEBVIEW2_DOWNLOAD_URL);
        let result = unsafe { ShellExecuteW(None, w!("open"), &url, None, None, SW_SHOWNORMAL) };
        if result.0 as isize <= 32 {
            log::error!(target: "desktop", "[webview2.download.open_failed] code={}", result.0 as isize);
        }
    }
}

/// Returns the bootstrap log used before Tauri's path resolver is available.
/// Current desktop delivery is Windows-only; future desktop platforms must add
/// their native data-directory implementation here instead of reading HOME or
/// other platform variables throughout the application.
pub(crate) fn emergency_log_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("com.cooliang.imail").join("logs").join("app.log"))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Opens an application-owned directory using the currently supported desktop
/// shell. Linux and macOS implementations belong to their future native
/// desktop phases and must be validated on those operating systems.
pub(crate) fn open_directory(path: &Path, label: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("打开{label}失败：{error}"))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(format!("当前桌面平台不支持打开{label}"))
    }
}

/// Resolves the historical Windows daemon root used only by the one-time
/// upgrade and NSIS uninstall cleanup paths.
pub(crate) fn legacy_local_service_root_from_environment() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(base) = std::env::var_os("IMAIL_SMOKE_LOCAL_APP_DATA") {
        return Ok(PathBuf::from(base)
            .join("com.cooliang.imail")
            .join("local-service"));
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| "无法确定当前用户本地数据目录".to_string())?;
        Ok(PathBuf::from(base)
            .join("com.cooliang.imail")
            .join("local-service"))
    }
    #[cfg(not(windows))]
    {
        Err("旧本地守护清理只支持 Windows".into())
    }
}
