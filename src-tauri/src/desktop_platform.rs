use std::path::{Path, PathBuf};

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
