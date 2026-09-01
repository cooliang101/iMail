use std::{collections::BTreeSet, path::Path, time::Duration};

use imail_protocol::CURRENT_SCHEMA_VERSION;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

const CURRENT_SCHEMA_SQL: &str = include_str!("../sql/schema-v10.sql");
const MIGRATION_V2_ACCOUNTS: &str = include_str!("../sql/migration-v2-accounts.sql");
const MIGRATION_V3_SYNC_FKS: &str = include_str!("../sql/migration-v3-sync-fks.sql");
const MIGRATION_V6_PUSH_FIRST: &str = include_str!("../sql/migration-v6-push-first.sql");
const MIGRATION_V11_MESSAGE_SOURCES: &str =
    include_str!("../sql/migration-v11-message-sources.sql");

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub from_version: u32,
    pub to_version: u32,
    pub applied_versions: Vec<u32>,
    pub quick_check: bool,
    pub foreign_keys_verified: bool,
}

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("找不到 iMail 数据库")]
    MissingDatabase,
    #[error("iMail 数据库 schema 版本无效")]
    InvalidSchema,
    #[error("数据库 schema v{actual} 高于当前 Rust 服务支持的 v{supported}")]
    FutureSchema { actual: u32, supported: u32 },
    #[error("迁移后 SQLite 完整性检查失败：{0}")]
    Integrity(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn migrate_database(
    database_path: impl AsRef<Path>,
) -> Result<MigrationReport, MigrationError> {
    let database_path = database_path.as_ref();
    if !database_path.is_file() {
        return Err(MigrationError::MissingDatabase);
    }
    let mut connection = Connection::open(database_path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", false)?;
    let migration = migrate_locked(&mut connection);
    let foreign_keys = connection.pragma_update(None, "foreign_keys", true);
    match (migration, foreign_keys) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(MigrationError::Sqlite(error)),
    }
}

fn migrate_locked(connection: &mut Connection) -> Result<MigrationReport, MigrationError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(CURRENT_SCHEMA_SQL)?;
    let from_version = schema_version(&transaction)?;
    if from_version > CURRENT_SCHEMA_VERSION {
        return Err(MigrationError::FutureSchema {
            actual: from_version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    let mut applied_versions = Vec::new();
    if from_version < 1 {
        migrate_legacy_columns(&transaction)?;
        applied_versions.push(1);
    }
    if from_version < 2 {
        migrate_account_email_constraint(&transaction)?;
        applied_versions.push(2);
    }
    if from_version < 3 {
        migrate_sync_foreign_keys(&transaction)?;
        applied_versions.push(3);
    }
    if from_version < 4 {
        if !columns(&transaction, "sync_jobs")?.contains("rerun_requested") {
            transaction.execute_batch(
                "ALTER TABLE sync_jobs ADD COLUMN rerun_requested INTEGER NOT NULL DEFAULT 0
                 CHECK (rerun_requested IN (0, 1));",
            )?;
        }
        applied_versions.push(4);
    }
    if from_version < 5 {
        if !columns(&transaction, "accounts")?.contains("proxy_json") {
            transaction.execute_batch("ALTER TABLE accounts ADD COLUMN proxy_json TEXT;")?;
        }
        applied_versions.push(5);
    }
    if from_version < 6 {
        migrate_push_first_policies(&transaction)?;
        applied_versions.push(6);
    }
    if from_version < 7 {
        applied_versions.push(7);
    }
    if from_version < 8 {
        applied_versions.push(8);
    }
    if from_version < 9 {
        if !columns(&transaction, "developer_tokens")?.contains("user_id") {
            transaction.execute_batch(
                "ALTER TABLE developer_tokens ADD COLUMN user_id TEXT NOT NULL DEFAULT '__legacy__';",
            )?;
        }
        applied_versions.push(9);
    }
    if from_version < 10 {
        applied_versions.push(10);
    }
    if from_version < 11 {
        transaction.execute_batch(MIGRATION_V11_MESSAGE_SOURCES)?;
        applied_versions.push(11);
    }
    if from_version < 12 {
        transaction.execute_batch(include_str!("../sql/migration-v12-composition.sql"))?;
        // Preserve historical reply relationships where exact source was already cached.
        // Stream one source at a time; neither rewrite nor fetch original messages.
        let mut statement =
            transaction.prepare("SELECT message_id, source FROM message_sources")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let source: Vec<u8> = row.get(1)?;
            if let Ok(parsed) = imail_mail::parse_rfc822(&source) {
                transaction.execute(
                    "UPDATE messages SET mail_headers_json=?1 WHERE id=?2",
                    (serde_json::to_string(&parsed.headers)?, id),
                )?;
            }
        }
        applied_versions.push(12);
    }
    if from_version < 13 {
        transaction.execute_batch(include_str!("../sql/migration-v13-search.sql"))?;
        applied_versions.push(13);
    }
    if from_version < 14 {
        transaction.execute_batch(include_str!("../sql/migration-v14-rules.sql"))?;
        applied_versions.push(14);
    }
    if from_version < 15 {
        transaction.execute_batch(include_str!("../sql/migration-v15-outbox.sql"))?;
        applied_versions.push(15);
    }
    ensure_message_query_indexes(&transaction)?;
    if from_version < CURRENT_SCHEMA_VERSION {
        transaction.execute(
            "INSERT INTO metadata (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [CURRENT_SCHEMA_VERSION.to_string()],
        )?;
    }
    let quick = transaction
        .prepare("PRAGMA quick_check")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if quick.as_slice() != ["ok"] {
        return Err(MigrationError::Integrity(quick.join("; ")));
    }
    let foreign_key_errors = transaction
        .prepare("PRAGMA foreign_key_check")?
        .query([])?
        .mapped(|_| Ok(()))
        .count();
    if foreign_key_errors != 0 {
        return Err(MigrationError::Integrity(format!(
            "foreign_key_check 发现 {foreign_key_errors} 项异常"
        )));
    }
    let report = MigrationReport {
        from_version,
        to_version: CURRENT_SCHEMA_VERSION,
        applied_versions,
        quick_check: true,
        foreign_keys_verified: true,
    };
    transaction.commit()?;
    Ok(report)
}

fn ensure_message_query_indexes(transaction: &Transaction<'_>) -> Result<(), rusqlite::Error> {
    transaction.execute_batch(
        "CREATE INDEX IF NOT EXISTS messages_account_role_date_id
           ON messages(account_id, mailbox_role, received_at DESC, id DESC);
         CREATE INDEX IF NOT EXISTS messages_account_mailbox_date_id
           ON messages(account_id, mailbox, received_at DESC, id DESC);
         CREATE INDEX IF NOT EXISTS messages_account_role_unread_date_id
           ON messages(account_id, mailbox_role, unread, received_at DESC, id DESC);",
    )
}

fn schema_version(transaction: &Transaction<'_>) -> Result<u32, MigrationError> {
    let raw: Option<String> = transaction
        .query_row(
            "SELECT value FROM metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(0);
    };
    let number = raw
        .trim()
        .parse::<f64>()
        .map_err(|_| MigrationError::InvalidSchema)?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > u32::MAX as f64 {
        return Err(MigrationError::InvalidSchema);
    }
    Ok(number as u32)
}

fn columns(
    transaction: &Transaction<'_>,
    table: &str,
) -> Result<BTreeSet<String>, rusqlite::Error> {
    transaction
        .prepare("SELECT name FROM pragma_table_info(?1)")?
        .query_map([table], |row| row.get(0))?
        .collect()
}

fn migrate_legacy_columns(transaction: &Transaction<'_>) -> Result<(), MigrationError> {
    let message_columns = columns(transaction, "messages")?;
    if !message_columns.contains("mailbox_role") {
        transaction.execute_batch(
            "ALTER TABLE messages ADD COLUMN mailbox_role TEXT NOT NULL DEFAULT 'inbox';",
        )?;
    }
    if !message_columns.contains("labels_json") {
        transaction.execute_batch(
            "ALTER TABLE messages ADD COLUMN labels_json TEXT NOT NULL DEFAULT '[]';",
        )?;
    }
    if !message_columns.contains("snoozed_until") {
        transaction.execute_batch("ALTER TABLE messages ADD COLUMN snoozed_until TEXT;")?;
    }
    let account_columns = columns(transaction, "accounts")?;
    if !account_columns.contains("user_id") {
        transaction.execute_batch(
            "ALTER TABLE accounts ADD COLUMN user_id TEXT NOT NULL DEFAULT '__legacy__';",
        )?;
    }
    if !account_columns.contains("mailboxes_json") {
        transaction.execute_batch(
            "ALTER TABLE accounts ADD COLUMN mailboxes_json TEXT NOT NULL DEFAULT '[]';",
        )?;
    }
    if !account_columns.contains("group_icon") {
        transaction.execute_batch(
            "ALTER TABLE accounts ADD COLUMN group_icon TEXT NOT NULL DEFAULT 'folder';",
        )?;
    }
    let draft_columns = columns(transaction, "drafts")?;
    if !draft_columns.contains("html_body") {
        transaction
            .execute_batch("ALTER TABLE drafts ADD COLUMN html_body TEXT NOT NULL DEFAULT '';")?;
    }
    if !draft_columns.contains("attachments_json") {
        transaction.execute_batch(
            "ALTER TABLE drafts ADD COLUMN attachments_json TEXT NOT NULL DEFAULT '[]';",
        )?;
    }
    if !columns(transaction, "developer_tokens")?.contains("user_id") {
        transaction.execute_batch(
            "ALTER TABLE developer_tokens ADD COLUMN user_id TEXT NOT NULL DEFAULT '__legacy__';",
        )?;
    }
    if !columns(transaction, "contacts")?.contains("user_id") {
        transaction.execute_batch(
            "ALTER TABLE contacts RENAME TO contacts_legacy_owner;
             CREATE TABLE contacts (
               user_id TEXT NOT NULL, address TEXT NOT NULL COLLATE NOCASE, name TEXT NOT NULL,
               message_count INTEGER NOT NULL, last_contact_at TEXT NOT NULL, logo_key TEXT,
               logo_content_type TEXT, logo_source_url TEXT, logo_fetched_at TEXT,
               PRIMARY KEY (user_id, address)
             ) STRICT;
             INSERT INTO contacts SELECT '__legacy__', address, name, message_count, last_contact_at,
               logo_key, logo_content_type, logo_source_url, logo_fetched_at FROM contacts_legacy_owner;
             DROP TABLE contacts_legacy_owner;
             CREATE INDEX contacts_last_contact ON contacts(user_id, last_contact_at DESC);",
        )?;
    }
    if !columns(transaction, "logo_fetch_attempts")?.contains("user_id") {
        transaction.execute_batch(
            "ALTER TABLE logo_fetch_attempts RENAME TO logo_fetch_attempts_legacy_owner;
             CREATE TABLE logo_fetch_attempts (
               user_id TEXT NOT NULL, target TEXT NOT NULL, domain_key TEXT NOT NULL,
               status TEXT NOT NULL, detail TEXT NOT NULL, attempted_at TEXT NOT NULL,
               PRIMARY KEY (user_id, target)
             ) STRICT;
             INSERT INTO logo_fetch_attempts SELECT '__legacy__', target, domain_key, status,
               detail, attempted_at FROM logo_fetch_attempts_legacy_owner;
             DROP TABLE logo_fetch_attempts_legacy_owner;
             CREATE INDEX logo_fetch_attempts_domain
               ON logo_fetch_attempts(user_id, domain_key, attempted_at DESC);",
        )?;
    }
    Ok(())
}

fn has_legacy_email_index(transaction: &Transaction<'_>) -> Result<bool, rusqlite::Error> {
    let mut statement = transaction
        .prepare("SELECT name FROM pragma_index_list('accounts') WHERE [unique]=1 ORDER BY name")?;
    let indexes = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for index in indexes {
        let names = transaction
            .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?
            .query_map([&index], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if names == ["email"] {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_account_email_constraint(transaction: &Transaction<'_>) -> Result<(), MigrationError> {
    if has_legacy_email_index(transaction)? {
        transaction.execute_batch(MIGRATION_V2_ACCOUNTS)?;
    }
    Ok(())
}

fn sync_tables_have_foreign_keys(transaction: &Transaction<'_>) -> Result<bool, rusqlite::Error> {
    for table in [
        "sync_policies",
        "mailbox_sync_states",
        "sync_jobs",
        "sync_events",
    ] {
        let found: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_list(?1)
                           WHERE [table]='accounts' AND [from]='account_id'
                             AND upper(on_delete)='CASCADE')",
            [table],
            |row| row.get(0),
        )?;
        if !found {
            return Ok(false);
        }
    }
    Ok(true)
}

fn migrate_sync_foreign_keys(transaction: &Transaction<'_>) -> Result<(), MigrationError> {
    if !sync_tables_have_foreign_keys(transaction)? {
        transaction.execute_batch(MIGRATION_V3_SYNC_FKS)?;
    }
    Ok(())
}

fn migrate_push_first_policies(transaction: &Transaction<'_>) -> Result<(), MigrationError> {
    if !columns(transaction, "sync_policies")?.contains("interval_minutes") {
        return Ok(());
    }
    transaction.execute_batch(MIGRATION_V6_PUSH_FIRST)?;
    let mut statement = transaction.prepare(
        "SELECT key, value FROM metadata WHERE key LIKE 'sync_default_policy:%' ORDER BY key",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for (key, raw) in rows {
        match serde_json::from_str::<Value>(&raw) {
            Ok(Value::Object(mut value)) => {
                value.remove("intervalMinutes");
                value.remove("syncOnStart");
                value.remove("retryOnRecovery");
                transaction.execute(
                    "UPDATE metadata SET value=?1 WHERE key=?2",
                    (serde_json::to_string(&value)?, key),
                )?;
            }
            _ => {
                transaction.execute("DELETE FROM metadata WHERE key=?1", [key])?;
            }
        }
    }
    Ok(())
}
