use std::{env, process};

use imail_core::ReadOnlyRepository;
use imail_security::MasterKey;
use imail_storage_sqlite::SqliteReadOnlyStore;

fn main() {
    let mut arguments = env::args_os().skip(1);
    let Some(data_dir) = arguments.next() else {
        eprintln!("用法：imail-db-inspect <iMail 数据目录> [--models|--digests|--credentials]");
        process::exit(2);
    };
    let mode = arguments.next();
    let data_dir = std::path::PathBuf::from(data_dir);
    let result = SqliteReadOnlyStore::open_data_dir(&data_dir).and_then(|store| {
        if mode.as_deref() == Some(std::ffi::OsStr::new("--models")) {
            store.read_snapshot().map(|snapshot| {
                serde_json::to_value(snapshot.counts()).expect("serialize model counts")
            })
        } else if mode.as_deref() == Some(std::ffi::OsStr::new("--digests")) {
            store
                .compatibility_digests()
                .map(|digests| serde_json::to_value(digests).expect("serialize digests"))
        } else if mode.as_deref() == Some(std::ffi::OsStr::new("--credentials")) {
            let key = MasterKey::from_file(data_dir.join("master.key"))
                .map_err(|_| imail_storage_sqlite::StorageError::InvalidMasterKey)?;
            store
                .credential_compatibility_summary(&key)
                .map(|summary| serde_json::to_value(summary).expect("serialize credentials"))
        } else {
            store
                .inventory()
                .map(|inventory| serde_json::to_value(inventory).expect("serialize inventory"))
        }
    });
    match result {
        Ok(output) => println!(
            "{}",
            serde_json::to_string(&output).expect("serialize output")
        ),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}
