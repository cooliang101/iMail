use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use imail_core::maintenance::DataMaintenancePort;
use imail_protocol::{DataBackupResult, DataRestoreResult, CURRENT_SCHEMA_VERSION};
use rusqlite::{Connection, DatabaseName, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("找不到 iMail 数据库")]
    MissingDatabase,
    #[error("备份或恢复目录不安全")]
    UnsafePath,
    #[error("目标目录已存在，拒绝覆盖")]
    TargetExists,
    #[error("备份内容无效或已损坏")]
    InvalidBackup,
    #[error("备份 schema v{actual} 高于当前支持的 v{supported}")]
    FutureSchema { actual: u32, supported: u32 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupReport {
    pub backup_root: PathBuf,
    pub schema_version: u32,
    pub file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub restore_root: PathBuf,
    pub schema_version: u32,
    pub integrity_manifest_verified: bool,
    pub master_key_included: bool,
    pub instance_id_included: bool,
    pub sender_logos_included: bool,
}

pub struct FilesystemDataMaintenance;

impl DataMaintenancePort for FilesystemDataMaintenance {
    type Error = BackupError;

    fn create_backup(
        &self,
        data_root: &str,
        backup_root: &str,
        service_version: &str,
        created_at: &str,
    ) -> Result<DataBackupResult, Self::Error> {
        let report = create_data_backup(data_root, backup_root, service_version, created_at)?;
        Ok(DataBackupResult {
            backup_root: report.backup_root.to_string_lossy().into_owned(),
            schema_version: report.schema_version,
            file_count: report.file_count,
        })
    }

    fn prepare_restore(
        &self,
        backup_root: &str,
        restore_root: &str,
    ) -> Result<DataRestoreResult, Self::Error> {
        let report = prepare_data_restore(backup_root, restore_root)?;
        Ok(DataRestoreResult {
            restore_root: report.restore_root.to_string_lossy().into_owned(),
            schema_version: report.schema_version,
            integrity_manifest_verified: report.integrity_manifest_verified,
            master_key_included: report.master_key_included,
            instance_id_included: report.instance_id_included,
            sender_logos_included: report.sender_logos_included,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    format_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    service_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema_version: Option<u32>,
    created_at: String,
    files: BTreeMap<String, String>,
}

pub fn create_data_backup(
    data_root: impl AsRef<Path>,
    backup_root: impl AsRef<Path>,
    service_version: &str,
    created_at: &str,
) -> Result<BackupReport, BackupError> {
    let data_root = canonical_directory(data_root.as_ref())?;
    let database_path = data_root.join("imail.sqlite");
    if !database_path.is_file() {
        return Err(BackupError::MissingDatabase);
    }
    let backup_root = absent_target(backup_root.as_ref())?;
    reject_overlap(&data_root, &backup_root)?;
    let staging = sibling_staging(&backup_root)?;
    fs::create_dir(&staging)?;
    let result = (|| {
        let source = Connection::open_with_flags(&database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let schema_version = schema_version(&source)?;
        source.backup(DatabaseName::Main, staging.join("imail.sqlite"), None)?;
        for name in ["master.key", "instance-id", "sender-logos"] {
            let source = data_root.join(name);
            if source.exists() {
                copy_entry(&source, &staging.join(name))?;
            }
        }
        let files = file_hashes(&staging)?;
        let manifest = BackupManifest {
            format_version: 2,
            service: Some("imail".into()),
            service_version: Some(service_version.to_string()),
            schema_version: Some(schema_version),
            created_at: created_at.to_string(),
            files,
        };
        let mut encoded = serde_json::to_vec_pretty(&manifest)?;
        encoded.push(b'\n');
        let manifest_path = staging.join("backup-manifest.json");
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        std::io::Write::write_all(&mut options.open(manifest_path)?, &encoded)?;
        fs::rename(&staging, &backup_root)?;
        Ok(BackupReport {
            backup_root,
            schema_version,
            file_count: manifest.files.len(),
        })
    })();
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

pub fn prepare_data_restore(
    backup_root: impl AsRef<Path>,
    restore_root: impl AsRef<Path>,
) -> Result<RestoreReport, BackupError> {
    let backup_root = canonical_directory(backup_root.as_ref())?;
    let restore_root = absent_target(restore_root.as_ref())?;
    reject_overlap(&backup_root, &restore_root)?;
    let database_path = backup_root.join("imail.sqlite");
    if !database_path.is_file() {
        return Err(BackupError::MissingDatabase);
    }
    let manifest_path = backup_root.join("backup-manifest.json");
    let manifest = if manifest_path.is_file() {
        let manifest: BackupManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        verify_manifest(&backup_root, &manifest)?;
        Some(manifest)
    } else {
        None
    };
    validate_optional_identity_files(&backup_root)?;
    let connection = Connection::open_with_flags(&database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    verify_database(&connection)?;
    let schema_version = schema_version(&connection)?;
    if schema_version > CURRENT_SCHEMA_VERSION {
        return Err(BackupError::FutureSchema {
            actual: schema_version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    if manifest.as_ref().is_some_and(|value| {
        value.format_version == 2 && value.schema_version != Some(schema_version)
    }) {
        return Err(BackupError::InvalidBackup);
    }
    drop(connection);
    let staging = sibling_staging(&restore_root)?;
    fs::create_dir(&staging)?;
    let result = (|| {
        for name in ["imail.sqlite", "master.key", "instance-id", "sender-logos"] {
            let source = backup_root.join(name);
            if source.exists() {
                copy_entry(&source, &staging.join(name))?;
            }
        }
        let copied = Connection::open_with_flags(
            staging.join("imail.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        verify_database(&copied)?;
        drop(copied);
        fs::rename(&staging, &restore_root)?;
        Ok(RestoreReport {
            restore_root,
            schema_version,
            integrity_manifest_verified: manifest.is_some(),
            master_key_included: backup_root.join("master.key").is_file(),
            instance_id_included: backup_root.join("instance-id").is_file(),
            sender_logos_included: backup_root.join("sender-logos").is_dir(),
        })
    })();
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn verify_manifest(root: &Path, manifest: &BackupManifest) -> Result<(), BackupError> {
    if !matches!(manifest.format_version, 1 | 2)
        || (manifest.format_version == 2
            && (manifest.service.as_deref() != Some("imail")
                || manifest
                    .service_version
                    .as_deref()
                    .map_or(true, str::is_empty)
                || manifest.schema_version.map_or(true, |version| version == 0)))
        || file_hashes(root)? != manifest.files
    {
        return Err(BackupError::InvalidBackup);
    }
    Ok(())
}

fn verify_database(connection: &Connection) -> Result<(), BackupError> {
    let quick: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if quick != "ok" {
        return Err(BackupError::InvalidBackup);
    }
    for table in ["accounts", "metadata"] {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(BackupError::InvalidBackup);
        }
    }
    Ok(())
}

fn schema_version(connection: &Connection) -> Result<u32, BackupError> {
    let raw: String = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| BackupError::InvalidBackup)?;
    raw.parse::<u32>()
        .ok()
        .filter(|version| *version > 0)
        .ok_or(BackupError::InvalidBackup)
}

fn validate_optional_identity_files(root: &Path) -> Result<(), BackupError> {
    let key = root.join("master.key");
    if key.exists() {
        let value = fs::read_to_string(key)?;
        if value.trim().len() != 64 || !value.trim().bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(BackupError::InvalidBackup);
        }
    }
    let instance = root.join("instance-id");
    if instance.exists() {
        let value = fs::read_to_string(instance)?;
        let id = Uuid::parse_str(value.trim()).map_err(|_| BackupError::InvalidBackup)?;
        if id.get_version_num() != 4 {
            return Err(BackupError::InvalidBackup);
        }
    }
    Ok(())
}

fn file_hashes(root: &Path) -> Result<BTreeMap<String, String>, BackupError> {
    let mut output = BTreeMap::new();
    walk_hashes(root, root, &mut output)?;
    Ok(output)
}

fn walk_hashes(
    root: &Path,
    current: &Path,
    output: &mut BTreeMap<String, String>,
) -> Result<(), BackupError> {
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        if current == root && entry.file_name() == "backup-manifest.json" {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(BackupError::InvalidBackup);
        }
        if metadata.is_dir() {
            walk_hashes(root, &path, output)?;
        } else if metadata.is_file() {
            let mut file = fs::File::open(&path)?;
            let mut hasher = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| BackupError::UnsafePath)?
                .to_string_lossy()
                .replace('\\', "/");
            output.insert(relative, format!("{:x}", hasher.finalize()));
        } else {
            return Err(BackupError::InvalidBackup);
        }
    }
    Ok(())
}

fn copy_entry(source: &Path, target: &Path) -> Result<(), BackupError> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(BackupError::InvalidBackup);
    }
    if metadata.is_dir() {
        fs::create_dir(target)?;
        let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            copy_entry(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        let options = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        let mut source = fs::File::open(source)?;
        let mut target = options;
        std::io::copy(&mut source, &mut target)?;
    } else {
        return Err(BackupError::InvalidBackup);
    }
    Ok(())
}

fn canonical_directory(path: &Path) -> Result<PathBuf, BackupError> {
    let path = fs::canonicalize(path)?;
    if !path.is_dir() {
        return Err(BackupError::UnsafePath);
    }
    Ok(path)
}

fn absent_target(path: &Path) -> Result<PathBuf, BackupError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if absolute.exists() {
        return Err(BackupError::TargetExists);
    }
    let name = absolute.file_name().ok_or(BackupError::UnsafePath)?;
    let parent = absolute.parent().ok_or(BackupError::UnsafePath)?;
    fs::create_dir_all(parent)?;
    Ok(fs::canonicalize(parent)?.join(name))
}

fn sibling_staging(target: &Path) -> Result<PathBuf, BackupError> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(BackupError::UnsafePath)?;
    let staging = target.with_file_name(format!("{name}.partial-{}", Uuid::new_v4()));
    if staging.exists() {
        return Err(BackupError::TargetExists);
    }
    Ok(staging)
}

fn reject_overlap(left: &Path, right: &Path) -> Result<(), BackupError> {
    if left.starts_with(right) || right.starts_with(left) {
        Err(BackupError::UnsafePath)
    } else {
        Ok(())
    }
}
