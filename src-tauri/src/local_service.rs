use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

#[cfg(not(windows))]
use std::time::{SystemTime, UNIX_EPOCH};

const LOCAL_SERVICE_HOST: &str = "127.0.0.1";
const LOCAL_SERVICE_PORT: u16 = 8787;
const WINDOWS_RUN_VALUE: &str = "iMailService";
const DELETE_LOCAL_DATA_CONFIRMATION: &str = "永久删除本地数据";
#[cfg(any(target_os = "macos", test))]
const MACOS_LAUNCH_LABEL: &str = "com.cooliang.imail.service";

#[derive(Clone, Deserialize, Serialize)]
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalServiceStatus {
    installed: bool,
    data_present: bool,
    enabled: bool,
    running: bool,
    state: &'static str,
    url: String,
    version: Option<String>,
    instance_id: Option<String>,
    error: Option<String>,
    diagnostic: Option<SupervisorDiagnostic>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupervisorDiagnostic {
    failures: u32,
    reason: String,
    exit_code: Option<i32>,
    updated_at_epoch_seconds: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceIdentity {
    service: String,
    instance_id: String,
    version: String,
    protocol_version: u32,
}

fn local_service_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("local-service"))
        .map_err(|error| format!("无法确定本地服务目录：{error}"))
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(local_service_root(app)?.join("daemon.json"))
}

fn sidecar_source() -> Result<PathBuf, String> {
    let directory = std::env::current_exe()
        .map_err(|error| format!("无法确定桌面程序位置：{error}"))?
        .parent()
        .ok_or_else(|| "桌面程序目录无效".to_string())?
        .to_path_buf();
    #[cfg(windows)]
    let path = directory.join("imail-service.exe");
    #[cfg(not(windows))]
    let path = directory.join("imail-service");
    if !path.is_file() {
        return Err(format!("安装包缺少本地服务程序：{}", path.display()));
    }
    Ok(path)
}

fn supervisor_source() -> Result<PathBuf, String> {
    let desktop =
        std::env::current_exe().map_err(|error| format!("无法确定桌面程序位置：{error}"))?;
    #[cfg(windows)]
    {
        let manager = desktop.with_file_name("imail-service-manager.exe");
        if !manager.is_file() {
            return Err(format!("安装包缺少本地服务管理程序：{}", manager.display()));
        }
        Ok(manager)
    }
    #[cfg(not(windows))]
    {
        Ok(desktop)
    }
}

fn read_config(path: &Path) -> Result<DaemonConfig, String> {
    let content =
        fs::read_to_string(path).map_err(|error| format!("读取本地服务配置失败：{error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("本地服务配置无效：{error}"))
}

fn validate_managed_config(root: &Path, config: &DaemonConfig) -> Result<(), String> {
    let executable_in_runtime = config.service_executable.parent()
        == Some(root.join("runtime").as_path())
        && config
            .service_executable
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.starts_with("imail-service-"));
    let lock_file_valid = config.supervisor_lock_file.as_os_str().is_empty()
        || config.supervisor_lock_file == root.join("supervisor.lock");
    #[cfg(debug_assertions)]
    let port_valid = config.port == LOCAL_SERVICE_PORT
        || (std::env::var_os("IMAIL_SMOKE_LOCAL_APP_DATA").is_some()
            && config.host == LOCAL_SERVICE_HOST);
    #[cfg(not(debug_assertions))]
    let port_valid = config.port == LOCAL_SERVICE_PORT;
    if !executable_in_runtime
        || config.data_dir != root.join("data")
        || config.control_file != root.join("control-token")
        || config.enabled_file != root.join("enabled")
        || config.log_file != root.join("logs").join("service.log")
        || !lock_file_valid
        || config.host != LOCAL_SERVICE_HOST
        || !port_valid
    {
        return Err("本地守护配置包含不受管理的路径或网络地址".into());
    }
    Ok(())
}

fn read_managed_config(root: &Path, path: &Path) -> Result<DaemonConfig, String> {
    let config = read_config(path)?;
    validate_managed_config(root, &config)?;
    Ok(config)
}

fn write_private(path: &Path, value: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建本地服务目录失败：{error}"))?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, value).map_err(|error| format!("写入本地服务配置失败：{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("设置本地服务文件权限失败：{error}"))?;
    }
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("替换本地服务配置失败：{error}"))?;
    }
    fs::rename(temporary, path).map_err(|error| format!("提交本地服务配置失败：{error}"))
}

fn command_value(executable: &Path, config: &Path) -> String {
    format!(
        "\"{}\" --imail-daemon \"{}\"",
        executable.display(),
        config.display()
    )
}

#[cfg(windows)]
fn register_startup(executable: &Path, config: &Path) -> Result<(), String> {
    let output = Command::new("reg.exe")
        .args([
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            WINDOWS_RUN_VALUE,
            "/t",
            "REG_SZ",
            "/d",
            &command_value(executable, config),
            "/f",
        ])
        .output()
        .map_err(|error| format!("注册用户级守护服务失败：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "注册用户级守护服务失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(windows)]
fn unregister_startup(_config: &Path) -> Result<(), String> {
    let output = Command::new("reg.exe")
        .args([
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            WINDOWS_RUN_VALUE,
            "/f",
        ])
        .output()
        .map_err(|error| format!("注销用户级守护服务失败：{error}"))?;
    if output.status.success() || output.status.code() == Some(1) {
        Ok(())
    } else {
        Err(format!(
            "注销用户级守护服务失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(target_os = "macos", test))]
fn launch_agent_contents(executable: &Path, config: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{MACOS_LAUNCH_LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>--imail-daemon</string><string>{}</string></array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
</dict></plist>
"#,
        xml_escape(&executable.to_string_lossy()),
        xml_escape(&config.to_string_lossy())
    )
}

#[cfg(target_os = "macos")]
fn launch_agent_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "无法确定当前用户目录".to_string())?;
    Ok(PathBuf::from(home)
        .join("Library/LaunchAgents")
        .join(format!("{MACOS_LAUNCH_LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn user_domain() -> Result<String, String> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .map_err(|error| format!("读取用户 ID 失败：{error}"))?;
    if !output.status.success() {
        return Err("读取用户 ID 失败".into());
    }
    Ok(format!(
        "gui/{}",
        String::from_utf8_lossy(&output.stdout).trim()
    ))
}

#[cfg(target_os = "macos")]
fn register_startup(executable: &Path, config: &Path) -> Result<(), String> {
    let plist = launch_agent_path()?;
    if let Some(parent) = plist.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建 LaunchAgents 目录失败：{error}"))?;
    }
    let content = launch_agent_contents(executable, config);
    write_private(&plist, &content)?;
    let domain = user_domain()?;
    let _ = Command::new("launchctl")
        .args(["bootout", &domain, &plist.to_string_lossy()])
        .output();
    let output = Command::new("launchctl")
        .args(["bootstrap", &domain, &plist.to_string_lossy()])
        .output()
        .map_err(|error| format!("加载 LaunchAgent 失败：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "加载 LaunchAgent 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "macos")]
fn unregister_startup(_config: &Path) -> Result<(), String> {
    let plist = launch_agent_path()?;
    if plist.exists() {
        let domain = user_domain()?;
        let _ = Command::new("launchctl")
            .args(["bootout", &domain, &plist.to_string_lossy()])
            .output();
        fs::remove_file(&plist).map_err(|error| format!("删除 LaunchAgent 失败：{error}"))?;
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn register_startup(_executable: &Path, _config: &Path) -> Result<(), String> {
    Err("当前平台尚不支持本地守护服务".into())
}
#[cfg(not(any(windows, target_os = "macos")))]
fn unregister_startup(_config: &Path) -> Result<(), String> {
    Ok(())
}

fn spawn_supervisor(executable: &Path, config: &Path) -> Result<(), String> {
    let mut command = Command::new(executable);
    command
        .args(["--imail-daemon", &config.to_string_lossy()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("启动用户级守护进程失败：{error}"))
}

async fn fetch_identity(config: &DaemonConfig) -> Result<ServiceIdentity, String> {
    let url = format!("http://{}:{}/api/system/info", config.host, config.port);
    let response = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("服务返回 {}", response.status().as_u16()));
    }
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取服务身份失败：{error}"))?;
    let identity = serde_json::from_str::<ServiceIdentity>(&body)
        .map_err(|error| format!("服务身份无效：{error}"))?;
    if identity.service != "imail" || identity.protocol_version != 1 {
        return Err("本地端口已被非兼容服务占用".into());
    }
    if identity.instance_id != config.instance_id {
        return Err("本地端口已被另一个 iMail 服务实例占用".into());
    }
    Ok(identity)
}

fn service_port_listening(config: &DaemonConfig) -> bool {
    let Ok(address) = format!("{}:{}", config.host, config.port).parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_ok()
}

async fn status_from_config(
    root: &Path,
    config: Option<DaemonConfig>,
    error: Option<String>,
) -> LocalServiceStatus {
    let url = format!("http://{LOCAL_SERVICE_HOST}:{LOCAL_SERVICE_PORT}");
    let data_present = root.join("data").is_dir();
    let Some(config) = config else {
        return LocalServiceStatus {
            installed: false,
            data_present,
            enabled: false,
            running: false,
            state: "notInstalled",
            url,
            version: None,
            instance_id: None,
            error,
            diagnostic: None,
        };
    };
    let installed = config.service_executable.is_file();
    let enabled = config.enabled_file.is_file();
    match fetch_identity(&config).await {
        Ok(identity) => LocalServiceStatus {
            installed,
            data_present,
            enabled,
            running: true,
            state: "running",
            url,
            version: Some(identity.version),
            instance_id: Some(identity.instance_id),
            error: None,
            diagnostic: None,
        },
        Err(reason) => {
            let diagnostic = if enabled {
                read_supervisor_diagnostic(&config)
            } else {
                None
            };
            let diagnostic_error = diagnostic.as_ref().map(|value| {
                let exit = value
                    .exit_code
                    .map(|code| format!("，退出码 {code}"))
                    .unwrap_or_default();
                format!(
                    "后台服务连续失败 {} 次{}；可打开日志目录查看详情",
                    value.failures, exit
                )
            });
            LocalServiceStatus {
                installed,
                data_present,
                enabled,
                running: false,
                state: if enabled && diagnostic.is_some() {
                    "error"
                } else if enabled {
                    "starting"
                } else {
                    "stopped"
                },
                url,
                version: None,
                instance_id: None,
                error: error
                    .or(diagnostic_error)
                    .or(if enabled { Some(reason) } else { None }),
                diagnostic,
            }
        }
    }
}

#[tauri::command]
pub async fn local_service_status(app: AppHandle) -> LocalServiceStatus {
    let root = match local_service_root(&app) {
        Ok(root) => root,
        Err(error) => {
            return status_from_config(
                Path::new("__imail_data_root_unavailable__"),
                None,
                Some(error),
            )
            .await;
        }
    };
    let path = match config_path(&app) {
        Ok(path) => path,
        Err(error) => return status_from_config(&root, None, Some(error)).await,
    };
    if !path.exists() {
        return status_from_config(&root, None, None).await;
    }
    match read_managed_config(&root, &path) {
        Ok(config) => status_from_config(&root, Some(config), None).await,
        Err(error) => status_from_config(&root, None, Some(error)).await,
    }
}

async fn rollback_activation(
    path: &Path,
    attempted: &DaemonConfig,
    previous: Option<&DaemonConfig>,
    previous_enabled: bool,
    supervisor_executable: &Path,
    executable_deployment: Option<&mut ExecutableDeployment>,
) -> String {
    let mut problems = Vec::new();
    let _ = fs::remove_file(&attempted.enabled_file);
    if fetch_identity(attempted).await.is_ok() {
        if let Err(error) = request_shutdown(attempted) {
            problems.push(error);
        }
        for _ in 0..20 {
            if fetch_identity(attempted).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    for _ in 0..20 {
        if supervisor_stopped(attempted, path) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if !supervisor_stopped(attempted, path) {
        problems.push("新代守护进程未能停止，未恢复旧配置".into());
        return problems.join("；");
    }
    if let Some(deployment) = executable_deployment {
        if let Err(error) = deployment.rollback() {
            problems.push(error);
            return problems.join("；");
        }
    }
    if let Err(error) = unregister_startup(path) {
        problems.push(error);
    }
    if let Some(previous) = previous {
        if let Err(error) = serde_json::to_string_pretty(previous)
            .map_err(|error| error.to_string())
            .and_then(|content| write_private(path, &content))
        {
            problems.push(format!("恢复旧守护配置失败：{error}"));
            return problems.join("；");
        }
        if previous_enabled {
            if let Err(error) = write_private(&previous.enabled_file, "enabled\n") {
                problems.push(format!("恢复旧服务启用状态失败：{error}"));
            } else {
                if let Err(error) = register_startup(supervisor_executable, path) {
                    problems.push(format!("恢复旧守护注册失败：{error}"));
                }
                if fetch_identity(previous).await.is_err() {
                    if let Err(error) = spawn_supervisor(supervisor_executable, path) {
                        problems.push(format!("恢复旧服务进程失败：{error}"));
                    }
                }
            }
        }
    } else if path.exists() {
        if let Err(error) = fs::remove_file(path) {
            problems.push(format!("清理失败的守护配置失败：{error}"));
        }
    }
    problems.join("；")
}

struct ExecutableDeployment {
    target: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    activated: bool,
    had_target: bool,
}

fn files_equal(left: &Path, right: &Path) -> Result<bool, String> {
    let left_metadata =
        fs::metadata(left).map_err(|error| format!("读取服务程序信息失败：{error}"))?;
    let right_metadata =
        fs::metadata(right).map_err(|error| format!("读取暂存服务程序信息失败：{error}"))?;
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }
    let mut left_file =
        fs::File::open(left).map_err(|error| format!("读取服务程序失败：{error}"))?;
    let mut right_file =
        fs::File::open(right).map_err(|error| format!("读取暂存服务程序失败：{error}"))?;
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left_file
            .read(&mut left_buffer)
            .map_err(|error| format!("校验服务程序失败：{error}"))?;
        let right_read = right_file
            .read(&mut right_buffer)
            .map_err(|error| format!("校验暂存服务程序失败：{error}"))?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "本地服务程序文件名无效".to_string())?;
    Ok(path.with_file_name(format!("{file_name}.{suffix}")))
}

fn stage_executable(source: &Path, target: &Path) -> Result<Option<ExecutableDeployment>, String> {
    let staged = sibling_with_suffix(target, "stage")?;
    let backup = sibling_with_suffix(target, "rollback")?;
    if backup.exists() {
        if !target.exists() {
            fs::rename(&backup, target)
                .map_err(|error| format!("恢复中断的服务程序切换失败：{error}"))?;
        } else if files_equal(source, target)? {
            fs::remove_file(&backup)
                .map_err(|error| format!("清理已完成的服务回滚文件失败：{error}"))?;
        } else {
            fs::remove_file(target)
                .map_err(|error| format!("清理中断的新服务程序失败：{error}"))?;
            fs::rename(&backup, target)
                .map_err(|error| format!("恢复中断的旧服务程序失败：{error}"))?;
        }
    }
    if target.is_file() && files_equal(source, target)? {
        return Ok(None);
    }
    if staged.exists() {
        fs::remove_file(&staged).map_err(|error| format!("清理旧服务暂存文件失败：{error}"))?;
    }
    fs::copy(source, &staged).map_err(|error| format!("暂存本地服务程序失败：{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = fs::set_permissions(&staged, fs::Permissions::from_mode(0o700)) {
            let _ = fs::remove_file(&staged);
            return Err(format!("设置暂存服务程序权限失败：{error}"));
        }
    }
    match files_equal(source, &staged) {
        Ok(true) => Ok(Some(ExecutableDeployment {
            target: target.to_path_buf(),
            staged,
            backup,
            activated: false,
            had_target: target.exists(),
        })),
        Ok(false) => {
            let _ = fs::remove_file(&staged);
            Err("暂存服务程序校验失败".into())
        }
        Err(error) => {
            let _ = fs::remove_file(&staged);
            Err(error)
        }
    }
}

impl ExecutableDeployment {
    fn activate(&mut self) -> Result<(), String> {
        if self.backup.exists() {
            return Err("存在未恢复的服务回滚文件，已取消激活".into());
        }
        if self.had_target {
            fs::rename(&self.target, &self.backup)
                .map_err(|error| format!("备份当前服务程序失败：{error}"))?;
        }
        if let Err(error) = fs::rename(&self.staged, &self.target) {
            if self.had_target {
                let _ = fs::rename(&self.backup, &self.target);
            }
            return Err(format!("激活本地服务程序失败：{error}"));
        }
        self.activated = true;
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), String> {
        if !self.activated {
            if self.staged.exists() {
                fs::remove_file(&self.staged)
                    .map_err(|error| format!("清理服务暂存文件失败：{error}"))?;
            }
            return Ok(());
        }
        if self.target.exists() {
            fs::remove_file(&self.target)
                .map_err(|error| format!("移除失败的新服务程序失败：{error}"))?;
        }
        if self.had_target {
            fs::rename(&self.backup, &self.target)
                .map_err(|error| format!("恢复旧服务程序失败：{error}"))?;
        }
        self.activated = false;
        Ok(())
    }

    fn commit(mut self) -> Result<(), String> {
        if self.staged.exists() {
            fs::remove_file(&self.staged)
                .map_err(|error| format!("清理服务暂存文件失败：{error}"))?;
        }
        if self.backup.exists() {
            fs::remove_file(&self.backup)
                .map_err(|error| format!("清理服务回滚文件失败：{error}"))?;
        }
        self.activated = false;
        Ok(())
    }
}

#[tauri::command]
pub async fn local_service_enable(app: AppHandle) -> Result<LocalServiceStatus, String> {
    let path = config_path(&app)?;
    let root = local_service_root(&app)?;
    let previous_config = if path.exists() {
        Some(read_managed_config(&root, &path)?)
    } else {
        None
    };
    let previous_enabled = previous_config
        .as_ref()
        .is_some_and(|value| value.enabled_file.exists());
    let runtime = root.join("runtime");
    let data_dir = root.join("data");
    let logs = root.join("logs");
    fs::create_dir_all(&runtime).map_err(|error| format!("创建服务运行目录失败：{error}"))?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("创建服务数据目录失败：{error}"))?;
    fs::create_dir_all(&logs).map_err(|error| format!("创建服务日志目录失败：{error}"))?;

    let source = sidecar_source()?;
    #[cfg(windows)]
    let executable = runtime.join(format!("imail-service-{}.exe", env!("CARGO_PKG_VERSION")));
    #[cfg(not(windows))]
    let executable = runtime.join(format!("imail-service-{}", env!("CARGO_PKG_VERSION")));
    let control_file = root.join("control-token");
    if !control_file.exists() {
        write_private(
            &control_file,
            &format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        )?;
    }
    let enabled_file = root.join("enabled");
    let instance_file = data_dir.join("instance-id");
    let instance_id = if instance_file.exists() {
        fs::read_to_string(&instance_file)
            .map_err(|error| format!("读取本地服务实例身份失败：{error}"))?
            .trim()
            .to_string()
    } else {
        let value = uuid::Uuid::new_v4().to_string();
        write_private(&instance_file, &format!("{value}\n"))?;
        value
    };
    uuid::Uuid::parse_str(&instance_id).map_err(|_| "本地服务实例身份无效".to_string())?;
    let mut config = DaemonConfig {
        service_executable: executable,
        data_dir,
        control_file,
        enabled_file,
        log_file: logs.join("service.log"),
        instance_id,
        supervisor_id: previous_config
            .as_ref()
            .map(|value| value.supervisor_id.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        supervisor_lock_file: root.join("supervisor.lock"),
        host: LOCAL_SERVICE_HOST.into(),
        port: LOCAL_SERVICE_PORT,
    };
    let supervisor_executable = supervisor_source()?;
    let current_identity = fetch_identity(&config).await;
    if current_identity.is_err() && service_port_listening(&config) {
        return Err(format!(
            "本地端口已被未知或不兼容服务占用：{}",
            current_identity
                .err()
                .unwrap_or_else(|| "身份检查失败".into())
        ));
    }
    let requires_upgrade = current_identity
        .as_ref()
        .is_ok_and(|identity| identity.version != env!("CARGO_PKG_VERSION"));
    let requires_supervisor_adoption = current_identity.is_ok()
        && previous_config
            .as_ref()
            .map_or(true, |value| value.supervisor_id.is_empty());
    if requires_upgrade || requires_supervisor_adoption {
        config.supervisor_id = uuid::Uuid::new_v4().to_string();
        if config.enabled_file.exists() {
            fs::remove_file(&config.enabled_file)
                .map_err(|error| format!("准备升级本地服务失败：{error}"))?;
        }
        let shutdown_config = previous_config.as_ref().unwrap_or(&config);
        if let Err(error) = request_shutdown(shutdown_config) {
            let rollback = rollback_activation(
                &path,
                &config,
                previous_config.as_ref(),
                previous_enabled,
                &supervisor_executable,
                None,
            )
            .await;
            return Err(activation_error(error, rollback));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if fetch_identity(&config).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if fetch_identity(&config).await.is_ok() {
            let error = "现有本地服务未能停止，升级已取消".to_string();
            let rollback = rollback_activation(
                &path,
                &config,
                previous_config.as_ref(),
                previous_enabled,
                &supervisor_executable,
                None,
            )
            .await;
            return Err(activation_error(error, rollback));
        }
        for _ in 0..20 {
            if supervisor_stopped(&config, &path) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if !supervisor_stopped(&config, &path) {
            let error = "旧版本守护进程未能停止，升级已取消".to_string();
            let rollback = rollback_activation(
                &path,
                &config,
                previous_config.as_ref(),
                previous_enabled,
                &supervisor_executable,
                None,
            )
            .await;
            return Err(activation_error(error, rollback));
        }
    }
    let mut executable_deployment = if current_identity.is_err() || requires_upgrade {
        match stage_executable(&source, &config.service_executable) {
            Ok(deployment) => deployment,
            Err(error) => {
                let rollback = rollback_activation(
                    &path,
                    &config,
                    previous_config.as_ref(),
                    previous_enabled,
                    &supervisor_executable,
                    None,
                )
                .await;
                return Err(activation_error(error, rollback));
            }
        }
    } else {
        None
    };
    if let Some(deployment) = executable_deployment.as_mut() {
        if let Err(error) = deployment.activate() {
            let rollback = rollback_activation(
                &path,
                &config,
                previous_config.as_ref(),
                previous_enabled,
                &supervisor_executable,
                Some(deployment),
            )
            .await;
            return Err(activation_error(error, rollback));
        }
    }
    let serialized = match serde_json::to_string_pretty(&config) {
        Ok(serialized) => serialized,
        Err(error) => {
            let rollback = rollback_activation(
                &path,
                &config,
                previous_config.as_ref(),
                previous_enabled,
                &supervisor_executable,
                executable_deployment.as_mut(),
            )
            .await;
            return Err(activation_error(error.to_string(), rollback));
        }
    };
    if let Err(error) = write_private(&path, &serialized) {
        let rollback = rollback_activation(
            &path,
            &config,
            previous_config.as_ref(),
            previous_enabled,
            &supervisor_executable,
            executable_deployment.as_mut(),
        )
        .await;
        return Err(activation_error(error, rollback));
    }
    if let Err(error) = write_private(&config.enabled_file, "enabled\n") {
        let rollback = rollback_activation(
            &path,
            &config,
            previous_config.as_ref(),
            previous_enabled,
            &supervisor_executable,
            executable_deployment.as_mut(),
        )
        .await;
        return Err(activation_error(error, rollback));
    }
    let _ = fs::remove_file(supervisor_diagnostic_path(&config));
    if let Err(error) = register_startup(&supervisor_executable, &path) {
        let rollback = rollback_activation(
            &path,
            &config,
            previous_config.as_ref(),
            previous_enabled,
            &supervisor_executable,
            executable_deployment.as_mut(),
        )
        .await;
        return Err(activation_error(error, rollback));
    }

    let service_already_running = fetch_identity(&config).await.is_ok();
    if !service_already_running {
        if let Err(error) = spawn_supervisor(&supervisor_executable, &path) {
            let rollback = rollback_activation(
                &path,
                &config,
                previous_config.as_ref(),
                previous_enabled,
                &supervisor_executable,
                executable_deployment.as_mut(),
            )
            .await;
            return Err(activation_error(error, rollback));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if fetch_identity(&config).await.is_ok() {
            if let Some(deployment) = executable_deployment.take() {
                let _ = deployment.commit();
            }
            return Ok(status_from_config(&root, Some(config), None).await);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let error = "本地服务启动超时，请查看服务日志".to_string();
    let rollback = rollback_activation(
        &path,
        &config,
        previous_config.as_ref(),
        previous_enabled,
        &supervisor_executable,
        executable_deployment.as_mut(),
    )
    .await;
    Err(activation_error(error, rollback))
}

fn activation_error(primary: String, rollback: String) -> String {
    if rollback.is_empty() {
        primary
    } else {
        format!("{primary}；自动回滚未完全成功：{rollback}")
    }
}

fn request_shutdown(config: &DaemonConfig) -> Result<(), String> {
    let token = fs::read_to_string(&config.control_file)
        .map_err(|error| format!("读取守护控制信息失败：{error}"))?;
    let address: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .map_err(|_| "本地服务地址无效".to_string())?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| format!("连接本地服务失败：{error}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let request = format!("POST /api/system/shutdown HTTP/1.1\r\nHost: {}:{}\r\nX-iMail-Daemon-Token: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", config.host, config.port, token.trim());
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送停服请求失败：{error}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("读取停服响应失败：{error}"))?;
    if response.starts_with("HTTP/1.1 202") {
        Ok(())
    } else {
        Err("本地服务拒绝停止请求".into())
    }
}

fn fetch_identity_blocking(config: &DaemonConfig) -> Result<ServiceIdentity, String> {
    let address: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .map_err(|_| "本地服务地址无效".to_string())?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| error.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    stream
        .write_all(
            format!(
                "GET /api/system/info HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
                config.host, config.port
            )
            .as_bytes(),
        )
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    if !response.starts_with("HTTP/1.1 200") {
        return Err("本地服务身份请求失败".into());
    }
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .ok_or_else(|| "本地服务身份响应无效".to_string())?;
    let identity = serde_json::from_str::<ServiceIdentity>(body)
        .map_err(|error| format!("本地服务身份无效：{error}"))?;
    if identity.service != "imail"
        || identity.protocol_version != 1
        || identity.instance_id != config.instance_id
    {
        return Err("本地服务身份不匹配".into());
    }
    Ok(identity)
}

fn cleanup_local_service_root(root: &Path) -> Result<(), String> {
    let cleanup_state = root.join("uninstall-state");
    let record_stage = |stage: &str| {
        let _ = write_private(&cleanup_state, stage);
    };
    record_stage("readingConfig");
    let path = root.join("daemon.json");
    let config = if path.exists() {
        Some(read_managed_config(root, &path)?)
    } else {
        None
    };
    record_stage("unregisteringStartup");
    if let Some(existing) = config.as_ref() {
        let _ = fs::remove_file(&existing.enabled_file);
        unregister_startup_for_cleanup(&path)?;
        record_stage("stoppingService");
        if fetch_identity_blocking(existing).is_ok() {
            request_shutdown(existing)?;
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if fetch_identity_blocking(existing).is_err() && supervisor_stopped(existing, &path) {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
        if fetch_identity_blocking(existing).is_ok() || !supervisor_stopped(existing, &path) {
            return Err("本地服务仍在运行，已取消卸载".into());
        }
    } else {
        unregister_startup_for_cleanup(&path)?;
    }
    record_stage("removingRuntime");
    for file in [
        "daemon.json",
        "control-token",
        "enabled",
        "supervisor.lock",
        "supervisor-status.json",
    ] {
        let target = root.join(file);
        if target.is_file() {
            fs::remove_file(&target).map_err(|error| format!("删除本地服务文件失败：{error}"))?;
        }
    }
    for directory in ["runtime", "logs"] {
        let target = root.join(directory);
        if target.is_dir() {
            fs::remove_dir_all(&target)
                .map_err(|error| format!("删除本地服务目录失败：{error}"))?;
        }
    }
    let _ = fs::remove_file(&cleanup_state);
    if root
        .read_dir()
        .is_ok_and(|mut entries| entries.next().is_none())
    {
        let _ = fs::remove_dir(root);
    }
    Ok(())
}

fn unregister_startup_for_cleanup(path: &Path) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if std::env::var("IMAIL_SMOKE_SKIP_STARTUP_REGISTRATION").as_deref() == Ok("true") {
        return Ok(());
    }
    unregister_startup(path)
}

fn environment_local_service_root() -> Result<PathBuf, String> {
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
        return Ok(PathBuf::from(base)
            .join("com.cooliang.imail")
            .join("local-service"));
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| "无法确定当前用户目录".to_string())?;
        return Ok(PathBuf::from(home)
            .join("Library/Application Support/com.cooliang.imail/local-service"));
    }
    #[allow(unreachable_code)]
    Err("当前平台不支持本地服务卸载清理".into())
}

pub fn run_uninstall_cleanup_from_args() -> Option<Result<(), String>> {
    if std::env::args_os().nth(1).as_deref()
        != Some(std::ffi::OsStr::new("--imail-uninstall-cleanup"))
    {
        return None;
    }
    Some(environment_local_service_root().and_then(|root| cleanup_local_service_root(&root)))
}

#[tauri::command]
pub async fn local_service_pause(app: AppHandle) -> Result<LocalServiceStatus, String> {
    let path = config_path(&app)?;
    let root = local_service_root(&app)?;
    let config = match read_managed_config(&root, &path) {
        Ok(config) => config,
        Err(_) => return Ok(status_from_config(&root, None, None).await),
    };
    if config.enabled_file.exists() {
        fs::remove_file(&config.enabled_file)
            .map_err(|error| format!("暂停本地服务失败：{error}"))?;
    }
    unregister_startup(&path)?;
    let shutdown_error = if fetch_identity(&config).await.is_ok() {
        request_shutdown(&config).err()
    } else {
        None
    };
    for _ in 0..20 {
        if fetch_identity(&config).await.is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    for _ in 0..20 {
        if supervisor_stopped(&config, &path) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let service_still_running = fetch_identity(&config).await.is_ok();
    let supervisor_still_running = !supervisor_stopped(&config, &path);
    if service_still_running || supervisor_still_running {
        let mut details = Vec::new();
        if service_still_running {
            details.push("API 进程仍在运行".to_string());
        }
        if supervisor_still_running {
            details.push("守护进程仍在运行".to_string());
        }
        if let Some(error) = shutdown_error {
            details.push(error);
        }
        return Err(format!("暂停本地服务超时：{}", details.join("；")));
    }
    Ok(status_from_config(&root, Some(config), None).await)
}

#[tauri::command]
pub async fn local_service_remove(app: AppHandle) -> Result<LocalServiceStatus, String> {
    let path = config_path(&app)?;
    let root = local_service_root(&app)?;
    let config = if path.exists() {
        Some(read_managed_config(&root, &path)?)
    } else {
        None
    };
    if let Some(existing) = config.as_ref() {
        if existing.enabled_file.exists() {
            fs::remove_file(&existing.enabled_file)
                .map_err(|error| format!("停用本地服务失败：{error}"))?;
        }
        unregister_startup(&path)?;
        if fetch_identity(existing).await.is_ok() {
            request_shutdown(existing)?;
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if fetch_identity(existing).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            if fetch_identity(existing).await.is_ok() {
                return Err("本地服务未能停止，未删除运行文件".into());
            }
        }
        for _ in 0..20 {
            if supervisor_stopped(existing, &path) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if !supervisor_stopped(existing, &path) {
            return Err("本地守护进程未能停止，未删除运行文件".into());
        }
    } else {
        unregister_startup(&path)?;
    }

    for file in [
        "daemon.json",
        "control-token",
        "enabled",
        "supervisor.lock",
        "supervisor-status.json",
    ] {
        let target = root.join(file);
        if target.is_file() {
            fs::remove_file(&target).map_err(|error| format!("删除本地服务文件失败：{error}"))?;
        }
    }
    for directory in ["runtime", "logs"] {
        let target = root.join(directory);
        if target.is_dir() {
            fs::remove_dir_all(&target)
                .map_err(|error| format!("删除本地服务目录失败：{error}"))?;
        }
    }
    Ok(status_from_config(&root, None, None).await)
}

fn delete_local_data_directory(root: &Path, confirmation: &str) -> Result<(), String> {
    if confirmation != DELETE_LOCAL_DATA_CONFIRMATION {
        return Err("删除确认文字不匹配".into());
    }
    for protected in ["daemon.json", "enabled", "supervisor.lock", "runtime"] {
        if root.join(protected).exists() {
            return Err("请先移除本地服务运行文件，再删除邮件数据".into());
        }
    }
    let data = root.join("data");
    let metadata = match fs::symlink_metadata(&data) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("读取本地数据目录失败：{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("本地数据路径不是受管理的普通目录".into());
    }
    fs::remove_dir_all(&data).map_err(|error| format!("删除本地邮件数据失败：{error}"))?;
    if root
        .read_dir()
        .is_ok_and(|mut entries| entries.next().is_none())
    {
        let _ = fs::remove_dir(root);
    }
    Ok(())
}

#[tauri::command]
pub async fn local_service_delete_data(
    app: AppHandle,
    confirmation: String,
) -> Result<LocalServiceStatus, String> {
    let root = local_service_root(&app)?;
    delete_local_data_directory(&root, &confirmation)?;
    Ok(status_from_config(&root, None, None).await)
}

#[cfg(any(not(windows), test))]
fn supervisor_service_args(config: &DaemonConfig) -> Vec<String> {
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

#[cfg(not(windows))]
fn supervisor_config_current(path: &Path, expected: &DaemonConfig) -> bool {
    read_config(path).is_ok_and(|current| {
        !expected.supervisor_id.is_empty() && current.supervisor_id == expected.supervisor_id
    })
}

fn supervisor_lock_path(config: &DaemonConfig, config_path: &Path) -> PathBuf {
    if config.supervisor_lock_file.as_os_str().is_empty() {
        config_path.with_file_name("supervisor.lock")
    } else {
        config.supervisor_lock_file.clone()
    }
}

fn supervisor_stopped(config: &DaemonConfig, config_path: &Path) -> bool {
    let lock_path = supervisor_lock_path(config, config_path);
    let Ok(file) = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock_path)
    else {
        return false;
    };
    if file.try_lock_exclusive().is_err() {
        return false;
    }
    let _ = file.unlock();
    true
}

#[cfg(not(windows))]
fn rotate_service_log(path: &Path) -> Result<(), String> {
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

fn supervisor_diagnostic_path(config: &DaemonConfig) -> PathBuf {
    config.control_file.with_file_name("supervisor-status.json")
}

fn read_supervisor_diagnostic(config: &DaemonConfig) -> Option<SupervisorDiagnostic> {
    let content = fs::read_to_string(supervisor_diagnostic_path(config)).ok()?;
    serde_json::from_str(&content).ok()
}

#[cfg(not(windows))]
fn write_supervisor_diagnostic(
    config: &DaemonConfig,
    failures: u32,
    reason: &str,
    exit_code: Option<i32>,
) {
    let diagnostic = SupervisorDiagnostic {
        failures,
        reason: reason.to_string(),
        exit_code,
        updated_at_epoch_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    if let Ok(content) = serde_json::to_string_pretty(&diagnostic) {
        let _ = write_private(&supervisor_diagnostic_path(config), &content);
    }
}

#[cfg(not(windows))]
fn wait_supervisor_backoff(config: &DaemonConfig, path: &Path, failures: u32) -> bool {
    let seconds = (1u64 << failures.min(5)).min(30);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if !config.enabled_file.exists() || !supervisor_config_current(path, config) {
            return false;
        }
        thread::sleep(Duration::from_millis(250));
    }
    true
}

#[tauri::command]
pub fn local_service_open_logs(app: AppHandle) -> Result<(), String> {
    let logs = local_service_root(&app)?.join("logs");
    fs::create_dir_all(&logs).map_err(|error| format!("创建服务日志目录失败：{error}"))?;
    #[cfg(windows)]
    {
        return Command::new("explorer.exe")
            .arg(&logs)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("打开服务日志目录失败：{error}"));
    }
    #[cfg(target_os = "macos")]
    {
        return Command::new("open")
            .arg(&logs)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("打开服务日志目录失败：{error}"));
    }
    #[allow(unreachable_code)]
    Err("当前平台不支持打开服务日志目录".into())
}

#[cfg(windows)]
pub fn run_daemon_from_args() -> bool {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.first().map(|value| value.as_os_str())
        != Some(std::ffi::OsStr::new("--imail-daemon"))
    {
        return false;
    }
    if let Ok(manager) = supervisor_source() {
        let mut command = Command::new(manager);
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        let _ = command.spawn();
    }
    true
}

#[cfg(not(windows))]
pub fn run_daemon_from_args() -> bool {
    let mut args = std::env::args_os();
    let _ = args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--imail-daemon")) {
        return false;
    }
    let Some(config_path) = args.next() else {
        return true;
    };
    let path = PathBuf::from(config_path);
    let Ok(root) = environment_local_service_root() else {
        return true;
    };
    if path != root.join("daemon.json") {
        return true;
    }
    let Ok(config) = read_managed_config(&root, &path) else {
        return true;
    };
    let lock_path = supervisor_lock_path(&config, &path);
    let Ok(supervisor_lock) = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock_path)
    else {
        return true;
    };
    if supervisor_lock.try_lock_exclusive().is_err() {
        return true;
    }
    let mut failures = 0u32;
    while config.enabled_file.exists() && supervisor_config_current(&path, &config) {
        if rotate_service_log(&config.log_file).is_err() {
            failures = failures.saturating_add(1);
            write_supervisor_diagnostic(&config, failures, "logRotationFailed", None);
            if !wait_supervisor_backoff(&config, &path, failures) {
                return true;
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
                write_supervisor_diagnostic(&config, failures, "logOpenFailed", None);
                if !wait_supervisor_backoff(&config, &path, failures) {
                    return true;
                }
                continue;
            }
        };
        let error_log = match log.try_clone() {
            Ok(file) => file,
            Err(_) => {
                failures = failures.saturating_add(1);
                write_supervisor_diagnostic(&config, failures, "logCloneFailed", None);
                if !wait_supervisor_backoff(&config, &path, failures) {
                    return true;
                }
                continue;
            }
        };
        let mut command = Command::new(&config.service_executable);
        command
            .args(supervisor_service_args(&config))
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
                write_supervisor_diagnostic(&config, failures, "serviceSpawnFailed", None);
                if !wait_supervisor_backoff(&config, &path, failures) {
                    return true;
                }
                continue;
            }
        };
        let started = Instant::now();
        let mut healthy_window_reached = false;
        let exit_status = loop {
            if !healthy_window_reached && started.elapsed() >= Duration::from_secs(5) {
                healthy_window_reached = true;
                let _ = fs::remove_file(supervisor_diagnostic_path(&config));
            }
            if !config.enabled_file.exists() || !supervisor_config_current(&path, &config) {
                let _ = request_shutdown(&config);
                let deadline = Instant::now() + Duration::from_secs(10);
                while Instant::now() < deadline {
                    if child.try_wait().ok().flatten().is_some() {
                        return true;
                    }
                    thread::sleep(Duration::from_millis(200));
                }
                let _ = child.kill();
                let _ = child.wait();
                return true;
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(500)),
                Err(_) => {
                    write_supervisor_diagnostic(
                        &config,
                        failures.saturating_add(1),
                        "serviceWaitFailed",
                        None,
                    );
                    return true;
                }
            }
        };
        failures = if started.elapsed() > Duration::from_secs(60) {
            0
        } else {
            failures.saturating_add(1)
        };
        write_supervisor_diagnostic(
            &config,
            failures.max(1),
            "serviceExited",
            exit_status.code(),
        );
        if !wait_supervisor_backoff(&config, &path, failures) {
            return true;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_the_supervisor_command_without_shell_expansion() {
        let value = command_value(
            Path::new(r"C:\Program Files\iMail\imail.exe"),
            Path::new(r"C:\Users\Me\iMail Data\daemon.json"),
        );
        assert_eq!(
            value,
            r#""C:\Program Files\iMail\imail.exe" --imail-daemon "C:\Users\Me\iMail Data\daemon.json""#
        );
    }

    #[test]
    fn builds_tokenized_service_arguments() {
        let config = DaemonConfig {
            service_executable: PathBuf::from("service"),
            data_dir: PathBuf::from("data dir"),
            control_file: PathBuf::from("token"),
            enabled_file: PathBuf::from("enabled"),
            log_file: PathBuf::from("log"),
            instance_id: "instance".into(),
            supervisor_id: "supervisor".into(),
            supervisor_lock_file: PathBuf::from("supervisor.lock"),
            host: "127.0.0.1".into(),
            port: 8787,
        };
        assert_eq!(
            supervisor_service_args(&config),
            vec![
                "--data-dir",
                "data dir",
                "--host",
                "127.0.0.1",
                "--port",
                "8787",
                "--daemon-control-file",
                "token"
            ]
        );
    }

    #[test]
    fn builds_safe_launch_agent_with_login_and_keepalive() {
        let content = launch_agent_contents(
            Path::new("/Applications/iMail & Work.app/Contents/MacOS/imail"),
            Path::new("/Users/me/Library/Application Support/<iMail>/daemon.json"),
        );
        assert!(content.contains("<key>RunAtLoad</key><true/>"));
        assert!(content.contains("<key>KeepAlive</key><true/>"));
        assert!(content.contains("<string>--imail-daemon</string>"));
        assert!(content.contains("iMail &amp; Work.app"));
        assert!(content.contains("&lt;iMail&gt;/daemon.json"));
        assert!(!content.contains("iMail & Work.app"));
    }

    #[test]
    fn executable_deployment_is_atomic_and_rollback_restores_previous_binary() {
        let directory = std::env::temp_dir().join(format!(
            "imail-executable-deployment-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.exe");
        let target = directory.join("target.exe");
        fs::write(&source, b"new-service-binary").unwrap();
        fs::write(&target, b"previous-service-binary").unwrap();

        let mut deployment = stage_executable(&source, &target).unwrap().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"previous-service-binary");
        assert_eq!(fs::read(&deployment.staged).unwrap(), b"new-service-binary");
        deployment.activate().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new-service-binary");
        assert_eq!(
            fs::read(&deployment.backup).unwrap(),
            b"previous-service-binary"
        );
        deployment.rollback().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"previous-service-binary");
        assert!(!deployment.staged.exists());
        assert!(!deployment.backup.exists());

        let mut deployment = stage_executable(&source, &target).unwrap().unwrap();
        deployment.activate().unwrap();
        let staged = deployment.staged.clone();
        let backup = deployment.backup.clone();
        deployment.commit().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new-service-binary");
        assert!(!staged.exists());
        assert!(!backup.exists());
        assert!(stage_executable(&source, &target).unwrap().is_none());

        let interrupted_backup = sibling_with_suffix(&target, "rollback").unwrap();
        fs::rename(&target, &interrupted_backup).unwrap();
        let recovered = stage_executable(&source, &target).unwrap();
        assert!(recovered.is_none());
        assert_eq!(fs::read(&target).unwrap(), b"new-service-binary");
        assert!(!interrupted_backup.exists());

        fs::write(&target, b"interrupted-new-binary").unwrap();
        fs::write(&interrupted_backup, b"known-good-binary").unwrap();
        let mut recovered = stage_executable(&source, &target).unwrap().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"known-good-binary");
        recovered.activate().unwrap();
        recovered.rollback().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"known-good-binary");

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn deletes_only_uninstalled_managed_data_after_exact_confirmation() {
        let root = std::env::temp_dir().join(format!(
            "imail-local-data-deletion-{}",
            uuid::Uuid::new_v4()
        ));
        let data = root.join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("imail.sqlite"), b"mail-data").unwrap();
        fs::write(root.join("preserve.txt"), b"unrelated-managed-state").unwrap();

        assert!(delete_local_data_directory(&root, "delete").is_err());
        assert!(data.exists());
        fs::write(root.join("daemon.json"), b"installed").unwrap();
        assert!(delete_local_data_directory(&root, DELETE_LOCAL_DATA_CONFIRMATION).is_err());
        assert!(data.exists());
        fs::remove_file(root.join("daemon.json")).unwrap();

        delete_local_data_directory(&root, DELETE_LOCAL_DATA_CONFIRMATION).unwrap();
        assert!(!data.exists());
        assert_eq!(
            fs::read(root.join("preserve.txt")).unwrap(),
            b"unrelated-managed-state"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
