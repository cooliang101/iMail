use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
};

use imail_core::{
    external_access::{ExternalAccessChanges, ExternalAccessService},
    AuthRepository, DeveloperTokenRepository,
};
use imail_http::{build_router, HttpAdapterConfig};
use imail_storage_sqlite::{migrate_database, SqliteAuthStore};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = fixture_data_dir()?;
    fs::create_dir_all(&data_dir)?;
    let data_dir = data_dir.canonicalize()?;
    let temp_dir = std::env::temp_dir().canonicalize()?;
    let safe_name = data_dir
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.starts_with("imail-mcp-sdk-"));
    if !data_dir.starts_with(&temp_dir) || !safe_name {
        return Err(
            "fixture data directory must be an imail-mcp-sdk-* child of the system temp directory"
                .into(),
        );
    }

    let key_path = data_dir.join("master.key");
    let mut key = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&key_path)?;
    key.write_all("52".repeat(32).as_bytes())?;
    key.sync_all()?;

    let database = data_dir.join("imail.sqlite");
    fs::File::create(&database)?;
    migrate_database(&database)?;
    let token = {
        let mut store = SqliteAuthStore::open_database(&database)?;
        let user = store.create_user(
            "mcp-sdk@example.com",
            "MCP SDK Fixture",
            "MCP SDK fixture password 123!",
        )?;
        ExternalAccessService::new(&mut store).update(
            &user.id,
            ExternalAccessChanges {
                gateway_enabled: None,
                mcp_enabled: Some(true),
            },
        )?;
        store
            .issue_developer_token(
                &user.id,
                "SDK interoperability",
                &["mcp:full".into()],
                &[],
                600,
            )?
            .raw
    };

    let mut config = HttpAdapterConfig::new(&data_dir);
    config.mcp = true;
    let router = build_router(config)?;
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    println!(
        "{}",
        json!({"url":format!("http://{address}/mcp"),"token":token})
    );
    std::io::stdout().flush()?;
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn fixture_data_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--data-dir")) {
        return Err("usage: imail-mcp-fixture-server --data-dir <temporary-directory>".into());
    }
    let directory = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("--data-dir requires a value")?;
    if arguments.next().is_some() {
        return Err("unexpected fixture server argument".into());
    }
    Ok(directory)
}
