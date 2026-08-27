use fs2::FileExt;
use imail_core::ReadOnlyRepository;
use imail_http::{EmbeddedServiceHost, HttpAdapterConfig};
use imail_security::MasterKey;
#[cfg(test)]
use imail_storage_sqlite::migrate_database;
use imail_storage_sqlite::{create_data_backup, SqliteReadOnlyStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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

use std::time::{SystemTime, UNIX_EPOCH};

const LOCAL_SERVICE_HOST: &str = "127.0.0.1";
#[cfg(any(test, feature = "legacy-daemon-admin"))]
const DEFAULT_LOCAL_SERVICE_PORT: u16 = 8787;
#[cfg(any(test, feature = "legacy-daemon-admin"))]
const MIN_LOCAL_SERVICE_PORT: u16 = 1024;
const WINDOWS_RUN_VALUE: &str = "iMailService";

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
#[cfg(feature = "legacy-daemon-admin")]
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
    #[cfg(feature = "legacy-daemon-admin")]
    version: String,
    protocol_version: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddedSwitchRecord {
    format_version: u32,
    state: &'static str,
    created_at: String,
    backup_root: PathBuf,
    database_sha256: String,
    schema_version: u32,
    account_count: u64,
    decrypted_credential_count: u64,
    legacy_runtime_retained: bool,
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

#[allow(dead_code)]
#[cfg(feature = "legacy-daemon-admin")]
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
    std::env::current_exe().map_err(|error| format!("无法确定桌面程序位置：{error}"))
}

fn read_config(path: &Path) -> Result<DaemonConfig, String> {
    let content =
        fs::read_to_string(path).map_err(|error| format!("读取本地服务配置失败：{error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("本地服务配置无效：{error}"))
}

#[cfg(any(test, feature = "legacy-daemon-admin"))]
fn validate_local_service_port(port: u16) -> Result<(), String> {
    if port < MIN_LOCAL_SERVICE_PORT {
        return Err(format!(
            "本地服务端口必须在 {MIN_LOCAL_SERVICE_PORT}–65535 之间"
        ));
    }
    Ok(())
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
    let port_valid = config.port >= 1024;
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

#[cfg(not(windows))]
fn register_startup(_executable: &Path, _config: &Path) -> Result<(), String> {
    Err("旧本地守护注册只支持 Windows".into())
}
#[cfg(not(windows))]
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

#[allow(dead_code)]
#[cfg(feature = "legacy-daemon-admin")]
fn service_port_listening(config: &DaemonConfig) -> bool {
    let Ok(address) = format!("{}:{}", config.host, config.port).parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_ok()
}

#[cfg(feature = "legacy-daemon-admin")]
async fn status_from_config(
    root: &Path,
    config: Option<DaemonConfig>,
    error: Option<String>,
) -> LocalServiceStatus {
    let url = format!(
        "http://{LOCAL_SERVICE_HOST}:{}",
        config
            .as_ref()
            .map(|value| value.port)
            .unwrap_or(DEFAULT_LOCAL_SERVICE_PORT)
    );
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
#[allow(dead_code)]
#[cfg(feature = "legacy-daemon-admin")]
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

#[allow(dead_code)]
#[cfg(feature = "legacy-daemon-admin")]
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

#[cfg(feature = "legacy-daemon-admin")]
struct ExecutableDeployment {
    target: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    activated: bool,
    had_target: bool,
}

#[cfg(feature = "legacy-daemon-admin")]
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

#[cfg(feature = "legacy-daemon-admin")]
fn service_binary_requires_refresh(source: &Path, target: &Path) -> Result<bool, String> {
    if !target.is_file() {
        return Ok(true);
    }
    files_equal(source, target).map(|equal| !equal)
}

#[cfg(feature = "legacy-daemon-admin")]
fn sibling_with_suffix(path: &Path, suffix: &str) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "本地服务程序文件名无效".to_string())?;
    Ok(path.with_file_name(format!("{file_name}.{suffix}")))
}

#[cfg(feature = "legacy-daemon-admin")]
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

#[cfg(feature = "legacy-daemon-admin")]
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
#[allow(dead_code)]
#[cfg(feature = "legacy-daemon-admin")]
pub async fn local_service_enable(
    app: AppHandle,
    port: Option<u16>,
) -> Result<LocalServiceStatus, String> {
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
    let selected_port = port
        .or_else(|| previous_config.as_ref().map(|value| value.port))
        .unwrap_or(DEFAULT_LOCAL_SERVICE_PORT);
    log::info!(target: "desktop", "[service.enable] requested port={selected_port}");
    validate_local_service_port(selected_port)?;
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
        port: selected_port,
    };
    let supervisor_executable = supervisor_source()?;
    let current_identity = fetch_identity(&config).await;
    if current_identity.is_err() && service_port_listening(&config) {
        return Err(format!(
            "本地端口 {selected_port} 已被未知或不兼容服务占用，请选择其他端口：{}",
            current_identity
                .err()
                .unwrap_or_else(|| "身份检查失败".into())
        ));
    }
    let port_changed = previous_config
        .as_ref()
        .is_some_and(|value| value.port != selected_port);
    let requires_upgrade = current_identity
        .as_ref()
        .is_ok_and(|identity| identity.version != env!("CARGO_PKG_VERSION"));
    let requires_binary_refresh =
        service_binary_requires_refresh(&source, &config.service_executable)?;
    let requires_supervisor_adoption = current_identity.is_ok()
        && previous_config
            .as_ref()
            .map_or(true, |value| value.supervisor_id.is_empty());
    if requires_upgrade || requires_binary_refresh || requires_supervisor_adoption || port_changed {
        config.supervisor_id = uuid::Uuid::new_v4().to_string();
        if config.enabled_file.exists() {
            fs::remove_file(&config.enabled_file)
                .map_err(|error| format!("准备切换本地服务配置失败：{error}"))?;
        }
        let shutdown_config = previous_config.as_ref().unwrap_or(&config);
        if fetch_identity(shutdown_config).await.is_ok() {
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
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if fetch_identity(shutdown_config).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if fetch_identity(shutdown_config).await.is_ok() {
            let error = "现有本地服务未能停止，配置切换已取消".to_string();
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
            if supervisor_stopped(shutdown_config, &path) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if !supervisor_stopped(shutdown_config, &path) {
            let error = "现有守护进程未能停止，配置切换已取消".to_string();
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
    let mut executable_deployment =
        if current_identity.is_err() || requires_upgrade || requires_binary_refresh {
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
            log::info!(target: "desktop", "[service.enabled] local service is healthy port={selected_port}");
            return Ok(status_from_config(&root, Some(config), None).await);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let error = "本地服务启动超时，请查看服务日志".to_string();
    log::error!(target: "desktop", "[service.enable_failed] startup timed out port={selected_port}");
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

#[allow(dead_code)]
#[cfg(feature = "legacy-daemon-admin")]
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
    let runtime = root.join("runtime");
    if runtime.is_dir() {
        fs::remove_dir_all(&runtime).map_err(|error| format!("删除本地服务目录失败：{error}"))?;
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

pub fn run_uninstall_cleanup_from_args() -> Option<Result<(), String>> {
    if std::env::args_os().nth(1).as_deref()
        != Some(std::ffi::OsStr::new("--imail-uninstall-cleanup"))
    {
        return None;
    }
    Some(
        crate::desktop_platform::legacy_local_service_root_from_environment()
            .and_then(|root| cleanup_local_service_root(&root)),
    )
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file =
        fs::File::open(path).map_err(|error| format!("读取切换保护文件失败：{error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("读取切换保护文件失败：{error}"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:X}", digest.finalize()))
}

fn completed_embedded_switch(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .and_then(|value| {
            value
                .get("state")
                .and_then(|state| state.as_str())
                .map(str::to_owned)
        })
        .as_deref()
        == Some("complete")
}

fn validate_embedded_switch_data(data_dir: &Path) -> Result<(u32, u64, u64), String> {
    let store = SqliteReadOnlyStore::open_data_dir(data_dir)
        .map_err(|error| format!("iMail 数据兼容检查失败：{error}"))?;
    let inventory = store
        .inventory()
        .map_err(|error| format!("iMail 数据完整性检查失败：{error}"))?;
    let key = MasterKey::from_file(data_dir.join("master.key"))
        .map_err(|error| format!("iMail 主密钥检查失败：{error}"))?;
    let credentials = store
        .credential_compatibility_summary(&key)
        .map_err(|error| format!("iMail 凭据解密检查失败：{error}"))?;
    if credentials.account_count != credentials.decrypted_count {
        return Err("iMail 凭据解密检查未覆盖全部账户".into());
    }
    if credentials.translation_credential_count != credentials.translation_decrypted_count {
        return Err("iMail 凭据解密检查未覆盖全部翻译服务".into());
    }
    let host = EmbeddedServiceHost::start(
        HttpAdapterConfig::production(data_dir.to_path_buf()).with_sync_worker(false),
    )
    .map_err(|error| format!("iMail 无网络首启检查失败：{error}"))?;
    let info = host.service_info();
    if info.get("service").and_then(serde_json::Value::as_str) != Some("imail")
        || info
            .get("capabilities")
            .and_then(|value| value.get("syncWorker"))
            .and_then(serde_json::Value::as_bool)
            != Some(false)
    {
        return Err("iMail 无网络首启返回了不兼容的服务身份".into());
    }
    host.shutdown(Duration::from_secs(10))
        .map_err(|error| format!("iMail 无网络首启关闭失败：{error}"))?;
    Ok((
        inventory.schema_version,
        credentials.account_count,
        credentials.decrypted_count,
    ))
}

async fn restore_legacy_after_switch_failure(
    config: &DaemonConfig,
    path: &Path,
) -> Result<(), String> {
    write_private(&config.enabled_file, "enabled\n")?;
    let supervisor = supervisor_source()?;
    register_startup(&supervisor, path)?;
    if fetch_identity(config).await.is_err() {
        spawn_supervisor(&supervisor, path)?;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if fetch_identity(config).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("旧本地服务恢复启动超时".into())
}

/// Performs the one-writer transition from the retained Node daemon to the
/// embedded Rust host. This intentionally retains all legacy runtime files and
/// creates a new, non-overwriting snapshot after the old writer has stopped.
pub(crate) async fn prepare_embedded_switch(app: &AppHandle) -> Result<PathBuf, String> {
    let root = local_service_root(app)?;
    let path = root.join("daemon.json");
    let record_path = root.join("embedded-switch.json");
    if completed_embedded_switch(&record_path) || !path.is_file() {
        return Ok(root);
    }
    let config = read_managed_config(&root, &path)?;
    let previous_enabled = config.enabled_file.is_file();
    let database = config.data_dir.join("imail.sqlite");
    let before_hash = file_sha256(&database)?;
    write_private(
        &record_path,
        &format!(
            "{{\n  \"formatVersion\": 1,\n  \"state\": \"stoppingLegacy\",\n  \"createdAt\": \"{}\",\n  \"legacyRuntimeRetained\": true\n}}\n",
            chrono::Utc::now().to_rfc3339()
        ),
    )?;

    let attempt = async {
        if previous_enabled {
            stop_legacy_service(app.clone()).await?;
        } else {
            for _ in 0..20 {
                if fetch_identity(&config).await.is_err() && supervisor_stopped(&config, &path) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            if fetch_identity(&config).await.is_ok() || !supervisor_stopped(&config, &path) {
                return Err("旧本地服务仍在运行，拒绝打开同一数据目录".to_string());
            }
        }

        let snapshots = root.join("migration-snapshots");
        fs::create_dir_all(&snapshots).map_err(|error| format!("创建切换快照目录失败：{error}"))?;
        let run_id = format!(
            "rust-switch-{}-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
            uuid::Uuid::new_v4().simple()
        );
        let backup_root = snapshots.join(run_id);
        let report = create_data_backup(
            &config.data_dir,
            &backup_root,
            env!("CARGO_PKG_VERSION"),
            &chrono::Utc::now().to_rfc3339(),
        )
        .map_err(|error| format!("创建切换前快照失败：{error}"))?;
        let (schema_version, account_count, decrypted_credential_count) =
            validate_embedded_switch_data(&config.data_dir)?;
        let after_hash = file_sha256(&database)?;
        if after_hash != before_hash {
            return Err("iMail 首启预检改变了数据库，旧服务保持停止并保留失败现场".into());
        }
        let record = EmbeddedSwitchRecord {
            format_version: 1,
            state: "complete",
            created_at: chrono::Utc::now().to_rfc3339(),
            backup_root: report.backup_root,
            database_sha256: after_hash,
            schema_version,
            account_count,
            decrypted_credential_count,
            legacy_runtime_retained: true,
        };
        let serialized = serde_json::to_string_pretty(&record)
            .map_err(|error| format!("编码切换记录失败：{error}"))?;
        write_private(&record_path, &format!("{serialized}\n"))?;
        Ok(root.clone())
    }
    .await;

    match attempt {
        Ok(root) => Ok(root),
        Err(primary) => {
            let unchanged = file_sha256(&database).is_ok_and(|hash| hash == before_hash);
            let rollback = if previous_enabled && unchanged {
                restore_legacy_after_switch_failure(&config, &path)
                    .await
                    .err()
            } else {
                None
            };
            let state = if rollback.is_none() && previous_enabled && unchanged {
                "rolledBack"
            } else {
                "failed"
            };
            let _ = write_private(
                &record_path,
                &format!(
                    "{{\n  \"formatVersion\": 1,\n  \"state\": \"{state}\",\n  \"createdAt\": \"{}\",\n  \"databaseUnchanged\": {unchanged},\n  \"legacyRuntimeRetained\": true\n}}\n",
                    chrono::Utc::now().to_rfc3339()
                ),
            );
            if let Some(rollback) = rollback {
                Err(format!("{primary}；恢复旧服务也失败：{rollback}"))
            } else {
                Err(primary)
            }
        }
    }
}

async fn stop_legacy_service(app: AppHandle) -> Result<(), String> {
    log::info!(target: "desktop", "[service.pause] requested");
    let path = config_path(&app)?;
    let root = local_service_root(&app)?;
    let config = match read_managed_config(&root, &path) {
        Ok(config) => config,
        Err(_) => return Ok(()),
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
    log::info!(target: "desktop", "[service.paused] local service stopped");
    Ok(())
}

#[tauri::command]
#[allow(dead_code)]
#[cfg(feature = "legacy-daemon-admin")]
pub async fn local_service_remove(app: AppHandle) -> Result<LocalServiceStatus, String> {
    log::info!(target: "desktop", "[service.remove] requested; user data will be preserved");
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
    log::info!(target: "desktop", "[service.removed] runtime and daemon registration removed");
    Ok(status_from_config(&root, None, None).await)
}

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
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
    else {
        return false;
    };
    if file.try_lock_exclusive().is_err() {
        return false;
    }
    let _ = fs2::FileExt::unlock(&file);
    true
}

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

#[cfg(feature = "legacy-daemon-admin")]
fn read_supervisor_diagnostic(config: &DaemonConfig) -> Option<SupervisorDiagnostic> {
    let content = fs::read_to_string(supervisor_diagnostic_path(config)).ok()?;
    serde_json::from_str(&content).ok()
}

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
#[allow(dead_code)]
#[cfg(feature = "legacy-daemon-admin")]
pub fn local_service_open_logs(app: AppHandle) -> Result<(), String> {
    let logs = local_service_root(&app)?.join("logs");
    fs::create_dir_all(&logs).map_err(|error| format!("创建服务日志目录失败：{error}"))?;
    log::info!(target: "desktop", "[logs.open] service log directory requested");
    crate::desktop_platform::open_directory(&logs, "服务日志目录")
}

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
    let Ok(root) = crate::desktop_platform::legacy_local_service_root_from_environment() else {
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
        .truncate(false)
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

    fn embedded_switch_fixture(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "imail-embedded-switch-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        let data = root.join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("imail.sqlite"), []).unwrap();
        migrate_database(data.join("imail.sqlite")).unwrap();
        fs::write(data.join("master.key"), MasterKey::generate_hex()).unwrap();
        fs::write(data.join("instance-id"), uuid::Uuid::new_v4().to_string()).unwrap();
        root
    }

    #[test]
    fn validates_embedded_switch_without_http_or_data_mutation() {
        let root = embedded_switch_fixture("valid");
        let database = root.join("data/imail.sqlite");
        let before = file_sha256(&database).unwrap();
        let (schema, accounts, decrypted) =
            validate_embedded_switch_data(&root.join("data")).unwrap();
        assert_eq!(schema, imail_protocol::CURRENT_SCHEMA_VERSION);
        assert_eq!((accounts, decrypted), (0, 0));
        assert_eq!(file_sha256(&database).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_master_key_fails_before_embedded_host_start() {
        let root = embedded_switch_fixture("bad-key");
        let database = root.join("data/imail.sqlite");
        let before = file_sha256(&database).unwrap();
        fs::write(root.join("data/master.key"), "not-a-key").unwrap();
        let error = validate_embedded_switch_data(&root.join("data")).unwrap_err();
        assert!(error.contains("master.key") || error.contains("主密钥"));
        assert_eq!(file_sha256(&database).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn switch_snapshot_refuses_overwrite_and_preserves_source() {
        let root = embedded_switch_fixture("backup-overwrite");
        let database = root.join("data/imail.sqlite");
        let before = file_sha256(&database).unwrap();
        let target = root.join("existing-backup");
        fs::create_dir(&target).unwrap();
        let error = create_data_backup(root.join("data"), &target, "test", "now").unwrap_err();
        assert!(error.to_string().contains("拒绝覆盖"));
        assert_eq!(file_sha256(&database).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

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
    fn accepts_user_selected_non_privileged_ports() {
        assert!(validate_local_service_port(DEFAULT_LOCAL_SERVICE_PORT).is_ok());
        assert!(validate_local_service_port(18787).is_ok());
        assert!(validate_local_service_port(443).is_err());
    }

    #[test]
    #[cfg(feature = "legacy-daemon-admin")]
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
    #[cfg(feature = "legacy-daemon-admin")]
    fn refreshes_a_changed_service_binary_even_when_the_app_version_is_unchanged() {
        let directory =
            std::env::temp_dir().join(format!("imail-service-refresh-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let bundled = directory.join("bundled.exe");
        let installed = directory.join("installed.exe");

        fs::write(&bundled, b"new-oauth-config").unwrap();
        assert!(service_binary_requires_refresh(&bundled, &installed).unwrap());
        fs::write(&installed, b"old-oauth-config").unwrap();
        assert!(service_binary_requires_refresh(&bundled, &installed).unwrap());
        fs::write(&installed, b"new-oauth-config").unwrap();
        assert!(!service_binary_requires_refresh(&bundled, &installed).unwrap());

        fs::remove_dir_all(directory).unwrap();
    }
}
