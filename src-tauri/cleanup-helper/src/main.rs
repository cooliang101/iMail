#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonConfig {
    service_executable: PathBuf,
    data_dir: PathBuf,
    control_file: PathBuf,
    enabled_file: PathBuf,
    log_file: PathBuf,
    instance_id: String,
    #[serde(default)]
    supervisor_id: String,
    #[serde(default)]
    supervisor_lock_file: PathBuf,
    host: String,
    port: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceIdentity {
    service: String,
    instance_id: String,
    protocol_version: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SupervisorDiagnostic {
    failures: u32,
    reason: String,
    exit_code: Option<i32>,
    updated_at_epoch_seconds: u64,
}

fn managed_root() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(base) = std::env::var_os("IMAIL_SMOKE_LOCAL_APP_DATA") {
        return Ok(PathBuf::from(base)
            .join("com.cooliang.imail")
            .join("local-service"));
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| "无法确定当前用户本地数据目录".to_string())?;
    Ok(PathBuf::from(base)
        .join("com.cooliang.imail")
        .join("local-service"))
}

fn validate_config(root: &Path, config: &DaemonConfig) -> Result<(), String> {
    let executable_valid = config.service_executable.parent()
        == Some(root.join("runtime").as_path())
        && config
            .service_executable
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.starts_with("imail-service-"));
    let lock_valid = config.supervisor_lock_file.as_os_str().is_empty()
        || config.supervisor_lock_file == root.join("supervisor.lock");
    #[cfg(debug_assertions)]
    let port_valid = config.port == 8787
        || (std::env::var_os("IMAIL_SMOKE_LOCAL_APP_DATA").is_some() && config.host == "127.0.0.1");
    #[cfg(not(debug_assertions))]
    let port_valid = config.port == 8787;
    if !executable_valid
        || config.data_dir != root.join("data")
        || config.control_file != root.join("control-token")
        || config.enabled_file != root.join("enabled")
        || config.log_file != root.join("logs").join("service.log")
        || !lock_valid
        || config.host != "127.0.0.1"
        || !port_valid
    {
        return Err("本地守护配置包含不受管理的路径或网络地址".into());
    }
    Ok(())
}

fn read_config(root: &Path, path: &Path) -> Result<DaemonConfig, String> {
    let content =
        fs::read_to_string(path).map_err(|error| format!("读取本地服务配置失败：{error}"))?;
    let config: DaemonConfig =
        serde_json::from_str(&content).map_err(|error| format!("本地服务配置无效：{error}"))?;
    validate_config(root, &config)?;
    Ok(config)
}

fn unregister_startup() -> Result<(), String> {
    #[cfg(debug_assertions)]
    if std::env::var("IMAIL_SMOKE_SKIP_STARTUP_REGISTRATION").as_deref() == Ok("true") {
        return Ok(());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let status = Command::new("reg.exe")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "iMailService",
                "/f",
            ])
            .creation_flags(0x08000000)
            .status()
            .map_err(|error| format!("注销用户级守护服务失败：{error}"))?;
        if status.success() || status.code() == Some(1) {
            return Ok(());
        }
        return Err("注销用户级守护服务失败".into());
    }
    #[allow(unreachable_code)]
    Ok(())
}

fn request(config: &DaemonConfig, request: &str) -> Result<String, String> {
    let address: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .map_err(|_| "本地服务地址无效".to_string())?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| error.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    Ok(response)
}

fn identity(config: &DaemonConfig) -> Result<ServiceIdentity, String> {
    let response = request(
        config,
        &format!(
            "GET /api/system/info HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
            config.host, config.port
        ),
    )?;
    if !response.starts_with("HTTP/1.1 200") {
        return Err("本地服务身份请求失败".into());
    }
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .ok_or_else(|| "本地服务身份响应无效".to_string())?;
    let value: ServiceIdentity =
        serde_json::from_str(body).map_err(|error| format!("本地服务身份无效：{error}"))?;
    if value.service != "imail"
        || value.protocol_version != 1
        || value.instance_id != config.instance_id
    {
        return Err("本地服务身份不匹配".into());
    }
    Ok(value)
}

fn shutdown(config: &DaemonConfig) -> Result<(), String> {
    let token = fs::read_to_string(&config.control_file)
        .map_err(|error| format!("读取守护控制信息失败：{error}"))?;
    let response = request(
        config,
        &format!(
            "POST /api/system/shutdown HTTP/1.1\r\nHost: {}:{}\r\nX-iMail-Daemon-Token: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            config.host,
            config.port,
            token.trim()
        ),
    )?;
    if response.starts_with("HTTP/1.1 202") {
        Ok(())
    } else {
        Err("本地服务拒绝停止请求".into())
    }
}

fn supervisor_stopped(root: &Path, config: &DaemonConfig) -> bool {
    let lock = if config.supervisor_lock_file.as_os_str().is_empty() {
        root.join("supervisor.lock")
    } else {
        config.supervisor_lock_file.clone()
    };
    let Ok(file) = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock)
    else {
        return false;
    };
    if file.try_lock_exclusive().is_err() {
        return false;
    }
    let _ = file.unlock();
    true
}

fn service_args(config: &DaemonConfig) -> Vec<String> {
    vec![
        "--data-dir".into(),
        config.data_dir.to_string_lossy().into_owned(),
        "--host".into(),
        config.host.clone(),
        "--port".into(),
        config.port.to_string(),
        "--daemon-control-file".into(),
        config.control_file.to_string_lossy().into_owned(),
    ]
}

fn config_current(path: &Path, expected: &DaemonConfig) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<DaemonConfig>(&content).ok())
        .is_some_and(|current| {
            !expected.supervisor_id.is_empty() && current.supervisor_id == expected.supervisor_id
        })
}

fn rotate_log(path: &Path) -> Result<(), String> {
    const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
    if fs::metadata(path).map(|value| value.len()).unwrap_or(0) < MAX_LOG_BYTES {
        return Ok(());
    }
    let previous = path.with_extension("log.1");
    if previous.exists() {
        fs::remove_file(&previous).map_err(|error| format!("轮转旧服务日志失败：{error}"))?;
    }
    fs::rename(path, previous).map_err(|error| format!("轮转服务日志失败：{error}"))
}

fn diagnostic_path(config: &DaemonConfig) -> PathBuf {
    config.control_file.with_file_name("supervisor-status.json")
}

fn write_diagnostic(config: &DaemonConfig, failures: u32, reason: &str, exit_code: Option<i32>) {
    let value = SupervisorDiagnostic {
        failures,
        reason: reason.to_string(),
        exit_code,
        updated_at_epoch_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    if let Ok(content) = serde_json::to_string_pretty(&value) {
        let temporary = diagnostic_path(config).with_extension("tmp");
        if fs::write(&temporary, content).is_ok() {
            let target = diagnostic_path(config);
            let _ = fs::remove_file(&target);
            let _ = fs::rename(temporary, target);
        }
    }
}

fn wait_backoff(config: &DaemonConfig, path: &Path, failures: u32) -> bool {
    let seconds = (1u64 << failures.min(5)).min(30);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if !config.enabled_file.exists() || !config_current(path, config) {
            return false;
        }
        thread::sleep(Duration::from_millis(250));
    }
    true
}

fn supervise(path: &Path) -> Result<(), String> {
    let root = managed_root()?;
    if path != root.join("daemon.json") {
        return Err("守护配置不属于当前用户的 iMail 目录".into());
    }
    let config = read_config(&root, path)?;
    let lock_path = if config.supervisor_lock_file.as_os_str().is_empty() {
        root.join("supervisor.lock")
    } else {
        config.supervisor_lock_file.clone()
    };
    let supervisor_lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|error| format!("打开守护锁失败：{error}"))?;
    if supervisor_lock.try_lock_exclusive().is_err() {
        return Ok(());
    }
    let mut failures = 0u32;
    while config.enabled_file.exists() && config_current(path, &config) {
        if rotate_log(&config.log_file).is_err() {
            failures = failures.saturating_add(1);
            write_diagnostic(&config, failures, "logRotationFailed", None);
            if !wait_backoff(&config, path, failures) {
                return Ok(());
            }
            continue;
        }
        let log = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.log_file)
        {
            Ok(file) => file,
            Err(_) => {
                failures = failures.saturating_add(1);
                write_diagnostic(&config, failures, "logOpenFailed", None);
                if !wait_backoff(&config, path, failures) {
                    return Ok(());
                }
                continue;
            }
        };
        let error_log = match log.try_clone() {
            Ok(file) => file,
            Err(_) => {
                failures = failures.saturating_add(1);
                write_diagnostic(&config, failures, "logCloneFailed", None);
                if !wait_backoff(&config, path, failures) {
                    return Ok(());
                }
                continue;
            }
        };
        let mut command = Command::new(&config.service_executable);
        command
            .args(service_args(&config))
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(error_log));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                failures = failures.saturating_add(1);
                write_diagnostic(&config, failures, "serviceSpawnFailed", None);
                if !wait_backoff(&config, path, failures) {
                    return Ok(());
                }
                continue;
            }
        };
        let started = Instant::now();
        let mut healthy_window_reached = false;
        let exit_status = loop {
            if !healthy_window_reached && started.elapsed() >= Duration::from_secs(5) {
                healthy_window_reached = true;
                let _ = fs::remove_file(diagnostic_path(&config));
            }
            if !config.enabled_file.exists() || !config_current(path, &config) {
                let _ = shutdown(&config);
                let deadline = Instant::now() + Duration::from_secs(10);
                while Instant::now() < deadline {
                    if child.try_wait().ok().flatten().is_some() {
                        return Ok(());
                    }
                    thread::sleep(Duration::from_millis(200));
                }
                let _ = child.kill();
                let _ = child.wait();
                return Ok(());
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(500)),
                Err(_) => {
                    write_diagnostic(
                        &config,
                        failures.saturating_add(1),
                        "serviceWaitFailed",
                        None,
                    );
                    return Ok(());
                }
            }
        };
        failures = if started.elapsed() > Duration::from_secs(60) {
            0
        } else {
            failures.saturating_add(1)
        };
        write_diagnostic(
            &config,
            failures.max(1),
            "serviceExited",
            exit_status.code(),
        );
        if !wait_backoff(&config, path, failures) {
            return Ok(());
        }
    }
    Ok(())
}

fn cleanup() -> Result<(), String> {
    let root = managed_root()?;
    let state = root.join("uninstall-state");
    let record = |value: &str| {
        let _ = fs::create_dir_all(&root);
        let _ = fs::write(&state, value);
    };
    record("readingConfig");
    let config_path = root.join("daemon.json");
    let config = if config_path.exists() {
        Some(read_config(&root, &config_path)?)
    } else {
        None
    };
    record("unregisteringStartup");
    if let Some(config) = config.as_ref() {
        let _ = fs::remove_file(&config.enabled_file);
        unregister_startup()?;
        record("stoppingService");
        if identity(config).is_ok() {
            shutdown(config)?;
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if identity(config).is_err() && supervisor_stopped(&root, config) {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
        if identity(config).is_ok() || !supervisor_stopped(&root, config) {
            return Err("本地服务仍在运行，已取消卸载".into());
        }
    } else {
        unregister_startup()?;
    }
    record("removingRuntime");
    for name in [
        "daemon.json",
        "control-token",
        "enabled",
        "supervisor.lock",
        "supervisor-status.json",
    ] {
        let target = root.join(name);
        if target.is_file() {
            fs::remove_file(target).map_err(|error| format!("删除本地服务文件失败：{error}"))?;
        }
    }
    for name in ["runtime", "logs"] {
        let target = root.join(name);
        if target.is_dir() {
            fs::remove_dir_all(target).map_err(|error| format!("删除本地服务目录失败：{error}"))?;
        }
    }
    #[cfg(debug_assertions)]
    if std::env::var("IMAIL_SMOKE_DELETE_DATA_AFTER_CLEANUP").as_deref() == Ok("true") {
        let data = root.join("data");
        if data.is_dir() {
            fs::remove_dir_all(data).map_err(|error| format!("删除冒烟测试数据失败：{error}"))?;
        }
    }
    let _ = fs::remove_file(state);
    if root
        .read_dir()
        .is_ok_and(|mut entries| entries.next().is_none())
    {
        let _ = fs::remove_dir(root);
    }
    Ok(())
}

fn main() {
    let mut args = std::env::args_os();
    let _ = args.next();
    let result = match args.next().as_deref() {
        Some(value) if value == std::ffi::OsStr::new("--imail-uninstall-cleanup") => cleanup(),
        Some(value) if value == std::ffi::OsStr::new("--imail-daemon") => args
            .next()
            .ok_or_else(|| "缺少守护配置路径".to_string())
            .and_then(|path| supervise(Path::new(&path))),
        _ => {
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(root: &Path) -> DaemonConfig {
        DaemonConfig {
            service_executable: root.join("runtime/imail-service-0.1.0.exe"),
            data_dir: root.join("data"),
            control_file: root.join("control-token"),
            enabled_file: root.join("enabled"),
            log_file: root.join("logs/service.log"),
            instance_id: "11111111-1111-4111-8111-111111111111".into(),
            supervisor_id: "22222222-2222-4222-8222-222222222222".into(),
            supervisor_lock_file: root.join("supervisor.lock"),
            host: "127.0.0.1".into(),
            port: 8787,
        }
    }

    #[test]
    fn accepts_only_the_fixed_user_service_boundary() {
        let root = Path::new(r"C:\Users\me\AppData\Local\com.cooliang.imail\local-service");
        let managed = config(root);
        assert!(validate_config(root, &managed).is_ok());

        let mut outside = config(root);
        outside.data_dir = PathBuf::from(r"C:\Users\me\Documents");
        assert!(validate_config(root, &outside).is_err());

        let mut exposed = config(root);
        exposed.host = "0.0.0.0".into();
        assert!(validate_config(root, &exposed).is_err());
    }
}
