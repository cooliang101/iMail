use std::{env, error::Error, fs, path::PathBuf, process};

use chrono::{SecondsFormat, Utc};
use imail_protocol::CURRENT_SCHEMA_VERSION;
use imail_storage_sqlite::{create_data_backup, migrate_database, prepare_data_restore};
use serde_json::{json, Value};

const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let data_root = env::var_os("IMAIL_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".data"));
    match execute(&arguments, data_root) {
        Ok(report) => println!(
            "{}",
            serde_json::to_string(&report).expect("serialize maintenance report")
        ),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}

fn execute(arguments: &[String], data_root: PathBuf) -> Result<Value, Box<dyn Error>> {
    match arguments {
        [command, backup_root] if command == "backup" => {
            let report = create_data_backup(
                data_root,
                backup_root,
                &service_version(),
                &Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            )?;
            Ok(json!({
                "ok": true,
                "backupRoot": report.backup_root,
                "schemaVersion": report.schema_version,
                "fileCount": report.file_count,
            }))
        }
        [command, backup_root, restore_root] if command == "restore" => {
            let report = prepare_data_restore(backup_root, restore_root)?;
            Ok(json!({
                "ok": true,
                "restoreRoot": report.restore_root,
                "databaseVerified": true,
                "integrityManifestVerified": report.integrity_manifest_verified,
                "schemaVersion": report.schema_version,
                "supportedSchemaVersion": CURRENT_SCHEMA_VERSION,
                "masterKeyIncluded": report.master_key_included,
                "instanceIdIncluded": report.instance_id_included,
                "senderLogosIncluded": report.sender_logos_included,
            }))
        }
        [command, backup_root, preflight_root] if command == "upgrade-preflight" => {
            let preflight_root = PathBuf::from(preflight_root);
            if preflight_root.exists() {
                return Err("目标目录已存在，拒绝覆盖".into());
            }
            let backup = create_data_backup(
                &data_root,
                backup_root,
                &service_version(),
                &Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            )?;
            let restored = prepare_data_restore(&backup.backup_root, &preflight_root)?;
            let migration = match migrate_database(preflight_root.join("imail.sqlite")) {
                Ok(report) => report,
                Err(error) => {
                    let _ = fs::remove_dir_all(&preflight_root);
                    return Err(error.into());
                }
            };
            Ok(json!({
                "ok": true,
                "activeDataUntouched": true,
                "backupRoot": backup.backup_root,
                "preflightRoot": preflight_root,
                "backupSchemaVersion": restored.schema_version,
                "migratedSchemaVersion": migration.to_version,
                "integrityManifestVerified": restored.integrity_manifest_verified,
                "sqliteQuickCheck": migration.quick_check,
                "foreignKeysVerified": migration.foreign_keys_verified,
            }))
        }
        _ => Err("用法：imail-maintenance backup <新备份目录> | restore <备份目录> <新恢复目录> | upgrade-preflight <新备份目录> <新预检目录>".into()),
    }
}

fn service_version() -> String {
    env::var("IMAIL_VERSION")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| SERVICE_VERSION.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use uuid::Uuid;

    #[test]
    fn backup_restore_and_preflight_are_non_overwriting() {
        let root = env::temp_dir().join(format!("imail-maintenance-{}", Uuid::new_v4()));
        let data = root.join("data");
        fs::create_dir_all(&data).unwrap();
        fs::File::create(data.join("imail.sqlite")).unwrap();
        migrate_database(data.join("imail.sqlite")).unwrap();
        let mut key = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(data.join("master.key"))
            .unwrap();
        key.write_all("52".repeat(32).as_bytes()).unwrap();

        let backup = root.join("backup");
        let restored = root.join("restored");
        let backup_report = execute(
            &["backup".into(), backup.to_string_lossy().into_owned()],
            data.clone(),
        )
        .unwrap();
        assert_eq!(backup_report["schemaVersion"], CURRENT_SCHEMA_VERSION);
        let restore_report = execute(
            &[
                "restore".into(),
                backup.to_string_lossy().into_owned(),
                restored.to_string_lossy().into_owned(),
            ],
            data.clone(),
        )
        .unwrap();
        assert_eq!(restore_report["integrityManifestVerified"], true);
        assert!(restored.join("master.key").is_file());
        assert!(execute(
            &["backup".into(), backup.to_string_lossy().into_owned()],
            data.clone(),
        )
        .is_err());

        let preflight_backup = root.join("preflight-backup");
        let preflight = root.join("preflight");
        let preflight_report = execute(
            &[
                "upgrade-preflight".into(),
                preflight_backup.to_string_lossy().into_owned(),
                preflight.to_string_lossy().into_owned(),
            ],
            data,
        )
        .unwrap();
        assert_eq!(preflight_report["activeDataUntouched"], true);
        assert_eq!(
            preflight_report["migratedSchemaVersion"],
            CURRENT_SCHEMA_VERSION
        );
        assert_eq!(preflight_report["foreignKeysVerified"], true);
        assert!(preflight.join("imail.sqlite").is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
