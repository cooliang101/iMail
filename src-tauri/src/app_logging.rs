use regex::Regex;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};

const MAX_APP_LOG_BYTES: u128 = 5 * 1024 * 1024;
const MAX_LOG_MESSAGE_CHARS: usize = 4_000;

fn email_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}").expect("valid email regex")
    })
}

fn bearer_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)\bbearer\s+[a-z0-9._~+/\-]+=*").expect("valid bearer regex")
    })
}

fn sensitive_header_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)(authorization|cookie|set-cookie)(\s*:\s*)[^\n]+")
            .expect("valid sensitive header regex")
    })
}

fn sensitive_field_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(
        r#"(?i)([\"']?(?:authorization|cookie|password|passwd|secret|token|encryptedsecret|client_secret|access_token|refresh_token)[\"']?)(\s*[=:]\s*)(?:\"[^\"]*\"|'[^']*'|[^\s,;&}]+)"#,
    ).expect("valid sensitive field regex"))
}

fn sensitive_query_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?i)([?&](?:code|state|token|access_token|refresh_token|client_secret)=)[^&\s]+",
        )
        .expect("valid sensitive query regex")
    })
}

pub fn sanitize_log_message(value: &str) -> String {
    let normalized = value.replace(['\0', '\r'], "");
    let redacted = email_pattern().replace_all(&normalized, "<email>");
    let redacted = sensitive_header_pattern().replace_all(&redacted, "${1}${2}<redacted>");
    let redacted = bearer_pattern().replace_all(&redacted, "Bearer <redacted>");
    let redacted = sensitive_field_pattern().replace_all(&redacted, "${1}${2}<redacted>");
    let redacted = sensitive_query_pattern().replace_all(&redacted, "${1}<redacted>");
    let mut output = redacted
        .chars()
        .take(MAX_LOG_MESSAGE_CHARS)
        .collect::<String>();
    if redacted.chars().count() > MAX_LOG_MESSAGE_CHARS {
        output.push_str("…<truncated>");
    }
    output
}

fn emergency_log_path() -> Option<PathBuf> {
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

fn write_emergency(level: &str, event: &str, message: &str) {
    let Some(path) = emergency_log_path() else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = writeln!(
        file,
        "[epoch={epoch}][{level}][bootstrap][{event}] {}",
        sanitize_log_message(message)
    );
}

pub fn prepare_desktop_process() {
    write_emergency(
        "INFO",
        "process.start",
        &format!(
            "iMail desktop process starting version={} os={} arch={}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
        ),
    );
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |details| {
        let payload = details
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| {
                details
                    .payload()
                    .downcast_ref::<String>()
                    .map(String::as_str)
            })
            .unwrap_or("unknown panic");
        let location = details
            .location()
            .map(|value| format!("{}:{}", value.file(), value.line()))
            .unwrap_or_else(|| "unknown".into());
        write_emergency(
            "ERROR",
            "process.panic",
            &format!("location={location} message={payload}"),
        );
        previous(details);
    }));
}

pub fn record_build_failure(error: &impl std::fmt::Display) {
    write_emergency("ERROR", "tauri.build_failed", &error.to_string());
}

pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::LogDir {
            file_name: Some("app".into()),
        }))
        .level(log::LevelFilter::Warn)
        .level_for("desktop", log::LevelFilter::Info)
        .level_for("frontend", log::LevelFilter::Info)
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .max_file_size(MAX_APP_LOG_BYTES)
        .rotation_strategy(RotationStrategy::KeepSome(3))
        .build()
}

#[tauri::command]
pub fn desktop_log(level: String, event: String, message: Option<String>) -> Result<(), String> {
    if event.is_empty()
        || event.len() > 64
        || !event
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-'))
    {
        return Err("日志事件名称无效".into());
    }
    let message = sanitize_log_message(message.as_deref().unwrap_or(""));
    match level.as_str() {
        "info" => log::info!(target: "frontend", "[{event}] {message}"),
        "warn" => log::warn!(target: "frontend", "[{event}] {message}"),
        "error" => log::error!(target: "frontend", "[{event}] {message}"),
        _ => return Err("日志级别无效".into()),
    }
    Ok(())
}

#[tauri::command]
pub fn desktop_open_app_logs(app: AppHandle) -> Result<(), String> {
    let logs = app
        .path()
        .app_log_dir()
        .map_err(|error| format!("读取应用日志目录失败：{error}"))?;
    fs::create_dir_all(&logs).map_err(|error| format!("创建应用日志目录失败：{error}"))?;
    log::info!(target: "desktop", "[logs.open] application log directory requested");
    #[cfg(windows)]
    {
        return std::process::Command::new("explorer.exe")
            .arg(&logs)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("打开应用日志目录失败：{error}"));
    }
    #[cfg(target_os = "macos")]
    {
        return std::process::Command::new("open")
            .arg(&logs)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("打开应用日志目录失败：{error}"));
    }
    #[allow(unreachable_code)]
    Err("当前平台不支持打开应用日志目录".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials_and_mail_addresses() {
        let input = "login alice@example.com Authorization: Bearer abc.def\nCookie: session=first; refresh=second\n{\"password\":\"hunter2\",\"refresh_token\":\"oauth-value\"}";
        let output = sanitize_log_message(input);
        assert!(!output.contains("alice@example.com"));
        assert!(!output.contains("abc.def"));
        assert!(!output.contains("hunter2"));
        assert!(!output.contains("oauth-value"));
        assert!(!output.contains("session=first"));
        assert!(!output.contains("refresh=second"));
        assert!(output.contains("<email>"));
        assert!(output.contains("<redacted>"));
    }

    #[test]
    fn redacts_sensitive_oauth_query_parameters() {
        let output = sanitize_log_message(
            "callback failed https://localhost/?code=secret-code&state=secret-state&safe=yes",
        );
        assert!(!output.contains("secret-code"));
        assert!(!output.contains("secret-state"));
        assert!(output.contains("safe=yes"));
    }

    #[test]
    fn truncates_unbounded_frontend_errors() {
        let output = sanitize_log_message(&"x".repeat(MAX_LOG_MESSAGE_CHARS + 100));
        assert!(output.ends_with("…<truncated>"));
        assert!(output.len() < MAX_LOG_MESSAGE_CHARS + 32);
    }
}
