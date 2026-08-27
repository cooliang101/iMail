use std::{collections::BTreeMap, env, error::Error, fs, path::PathBuf, time::Duration};

use imail_core::ReadOnlyRepository;
use imail_http::{EmbeddedServiceHost, HttpAdapterConfig};
use imail_security::MasterKey;
use imail_storage_sqlite::SqliteReadOnlyStore;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    files: BTreeMap<String, String>,
}

#[tokio::main]
async fn main() {
    match run() {
        Ok(report) => println!("{}", serde_json::to_string(&report).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<serde_json::Value, Box<dyn Error>> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    execute(&arguments)
}

fn execute(arguments: &[std::ffi::OsString]) -> Result<serde_json::Value, Box<dyn Error>> {
    let [data_dir, flag, manifest_path] = arguments else {
        return Err(
            "用法：imail-embedded-preflight <离线数据副本> --manifest <backup-manifest.json>"
                .into(),
        );
    };
    if flag != "--manifest" {
        return Err("必须显式提供 --manifest".into());
    }
    let data_dir = fs::canonicalize(PathBuf::from(data_dir))?;
    let manifest_path = fs::canonicalize(PathBuf::from(manifest_path))?;
    if !data_dir.is_dir()
        || manifest_path.file_name().and_then(|value| value.to_str())
            != Some("backup-manifest.json")
    {
        return Err("预检数据目录或备份清单无效".into());
    }
    if data_dir.file_name().and_then(|value| value.to_str()) == Some(".data")
        || data_dir
            .parent()
            .is_some_and(|parent| parent.join("enabled").is_file())
    {
        return Err("拒绝在活动数据目录上运行嵌入式预检".into());
    }
    let database = data_dir.join("imail.sqlite");
    let master_key_path = data_dir.join("master.key");
    let instance_id_path = data_dir.join("instance-id");
    if !database.is_file() || !master_key_path.is_file() || !instance_id_path.is_file() {
        return Err("预检副本缺少数据库、主密钥或实例身份".into());
    }
    let manifest: BackupManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    let database_hash_before = sha256_file(&database)?;
    if manifest.files.get("imail.sqlite") != Some(&database_hash_before) {
        return Err("预检数据库与备份清单不一致".into());
    }
    let master_key_hash_before = sha256_file(&master_key_path)?;
    let instance_id_hash_before = sha256_file(&instance_id_path)?;

    let store = SqliteReadOnlyStore::open_data_dir(&data_dir)?;
    let inventory = store.inventory()?;
    let counts = store.read_snapshot()?.counts();
    let digests = store.compatibility_digests()?;
    let key = MasterKey::from_file(&master_key_path)?;
    let credentials = store.credential_compatibility_summary(&key)?;

    let host = EmbeddedServiceHost::start(
        HttpAdapterConfig::production(&data_dir)
            .with_sync_worker(false)
            .with_apple_hme_keepalive(false),
    )?;
    let service_info = host.service_info();
    host.shutdown(Duration::from_secs(2))?;
    drop(host);

    let database_hash_after = sha256_file(&database)?;
    let master_key_hash_after = sha256_file(&master_key_path)?;
    let instance_id_hash_after = sha256_file(&instance_id_path)?;
    let unchanged = database_hash_before == database_hash_after
        && master_key_hash_before == master_key_hash_after
        && instance_id_hash_before == instance_id_hash_after;
    if !unchanged {
        return Err("嵌入式预检修改了受保护的副本文件".into());
    }
    Ok(json!({
        "ok":true,
        "noHttpListener":true,
        "syncWorker":service_info["capabilities"]["syncWorker"],
        "serviceInfo":service_info,
        "inventory":inventory,
        "modelCounts":counts,
        "digests":digests,
        "credentialSummary":credentials,
        "databaseSha256":database_hash_after,
        "protectedFilesUnchanged":unchanged,
    }))
}

fn sha256_file(path: &std::path::Path) -> Result<String, std::io::Error> {
    let bytes = fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use imail_storage_sqlite::{create_data_backup, migrate_database, prepare_data_restore};
    use std::ffi::OsString;
    use uuid::Uuid;

    struct Fixture {
        root: PathBuf,
        data: PathBuf,
        backup: PathBuf,
        preflight: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = env::temp_dir().join(format!("imail-embedded-preflight-{}", Uuid::new_v4()));
            let data = root.join("source").join("data");
            fs::create_dir_all(&data).unwrap();
            fs::File::create(data.join("imail.sqlite")).unwrap();
            migrate_database(data.join("imail.sqlite")).unwrap();
            fs::write(data.join("master.key"), "61".repeat(32)).unwrap();
            fs::write(data.join("instance-id"), format!("{}\n", Uuid::new_v4())).unwrap();
            let backup = root.join("backup");
            create_data_backup(&data, &backup, "test", "2026-08-11T00:00:00.000Z").unwrap();
            let preflight = root.join("preflight");
            prepare_data_restore(&backup, &preflight).unwrap();
            Self {
                root,
                data,
                backup,
                preflight,
            }
        }

        fn arguments(&self, data: &std::path::Path) -> Vec<OsString> {
            vec![
                data.as_os_str().to_owned(),
                "--manifest".into(),
                self.backup.join("backup-manifest.json").into_os_string(),
            ]
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn boots_the_embedded_host_without_network_or_protected_file_changes() {
        let fixture = Fixture::new();
        let report = execute(&fixture.arguments(&fixture.preflight)).unwrap();
        assert_eq!(report["ok"], true);
        assert_eq!(report["noHttpListener"], true);
        assert_eq!(report["syncWorker"], false);
        assert_eq!(report["protectedFilesUnchanged"], true);
        assert_eq!(report["inventory"]["schemaVersion"], 10);
    }

    #[test]
    fn rejects_active_directories_and_copies_that_do_not_match_the_manifest() {
        let fixture = Fixture::new();
        let enabled = fixture.data.parent().unwrap().join("enabled");
        fs::write(&enabled, "enabled\n").unwrap();
        assert!(execute(&fixture.arguments(&fixture.data))
            .unwrap_err()
            .to_string()
            .contains("活动数据目录"));
        fs::remove_file(enabled).unwrap();

        fs::write(fixture.preflight.join("imail.sqlite"), b"tampered").unwrap();
        assert!(execute(&fixture.arguments(&fixture.preflight))
            .unwrap_err()
            .to_string()
            .contains("备份清单"));
    }
}
