use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use imail_core::ReadOnlyRepository;
use imail_protocol::{
    AccountReadModel, ContactReadModel, CredentialCompatibilitySummary, DatabaseInventory,
    DeveloperTokenReadModel, DraftReadModel, MailboxSyncStateReadModel, MessageReadModel,
    ReadOnlySnapshot, SyncJobReadModel, SyncPolicyReadModel, CURRENT_SCHEMA_VERSION,
};
use imail_security::MasterKey;
use rusqlite::{types::Type, Connection, OpenFlags, Row};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

mod app_data;
mod apple_hme_addresses;
mod apple_hme_sessions;
mod auth;
mod backup;
mod content_data;
mod credential_codec;
mod developer_tokens;
mod export_encryption;
mod migration;
mod privacy_data;
mod sync_runtime;
mod translation_cache;
mod translation_providers;
pub use apple_hme_addresses::{AppleHmeAddressRecord, AppleHmeAddressSnapshot};
pub use apple_hme_sessions::AppleHmeSessionRecord;
pub use auth::{AuthStoreError, SqliteAuthStore};
pub use backup::{
    create_data_backup, prepare_data_restore, BackupReport, FilesystemDataMaintenance,
    RestoreReport,
};
pub use credential_codec::MasterKeyCredentialCodec;
pub use export_encryption::PortableAuthorizationExportEncryptor;
pub use migration::{migrate_database, MigrationError, MigrationReport};
pub use sync_runtime::{
    SyncCompletion, SyncEnqueue, SyncFailure, SyncPolicySettings, SyncRuntimeError,
    SyncRuntimeStore,
};

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("找不到 iMail 数据库：{0}")]
    MissingDatabase(PathBuf),
    #[error("数据目录缺少有效的 32 字节十六进制 master.key")]
    InvalidMasterKey,
    #[error("数据库 schema v{actual} 高于当前 Rust 服务支持的 v{supported}")]
    UnsupportedSchema { actual: u32, supported: u32 },
    #[error("iMail 数据库 schema 版本无效")]
    InvalidSchema,
    #[error("SQLite 完整性检查失败：{0}")]
    Integrity(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

pub struct SqliteReadOnlyStore {
    connection: Connection,
}

impl SqliteReadOnlyStore {
    pub fn open_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let data_dir = data_dir.as_ref();
        let database_path = data_dir.join("imail.sqlite");
        if !database_path.is_file() {
            return Err(StorageError::MissingDatabase(database_path));
        }
        let master_key = fs::read_to_string(data_dir.join("master.key"))
            .map_err(|_| StorageError::InvalidMasterKey)?;
        let key = master_key.trim();
        if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(StorageError::InvalidMasterKey);
        }
        Self::open_database(database_path)
    }

    pub fn open_database(database_path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let database_path = database_path.as_ref();
        if !database_path.is_file() {
            return Err(StorageError::MissingDatabase(database_path.to_path_buf()));
        }
        let connection = Connection::open_with_flags(
            database_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.pragma_update(None, "query_only", true)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let store = Self { connection };
        store.ensure_supported_schema()?;
        Ok(store)
    }

    fn schema_version(&self) -> Result<u32, StorageError> {
        let raw: String = self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => StorageError::InvalidSchema,
                other => StorageError::Sqlite(other),
            })?;
        raw.parse::<u32>().map_err(|_| StorageError::InvalidSchema)
    }

    fn ensure_supported_schema(&self) -> Result<u32, StorageError> {
        let actual = self.schema_version()?;
        if actual > CURRENT_SCHEMA_VERSION {
            return Err(StorageError::UnsupportedSchema {
                actual,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        Ok(actual)
    }

    fn list_accounts(&self) -> Result<Vec<AccountReadModel>, StorageError> {
        collect(&self.connection, "SELECT id, user_id, provider, email, display_name, group_name, group_icon, color, settings_json, proxy_json, auth_method, created_at, last_sync_at, status, last_error, mailboxes_json FROM accounts ORDER BY created_at, id", |row| Ok(AccountReadModel {
            id: row.get(0)?, owner_id: row.get(1)?, provider: row.get(2)?, email: row.get(3)?,
            display_name: row.get(4)?, group: row.get(5)?, group_icon: row.get(6)?, color: row.get(7)?,
            settings: json(row, 8)?, proxy: optional_json(row, 9)?, auth_method: row.get(10)?,
            created_at: row.get(11)?, last_sync_at: row.get(12)?, status: row.get(13)?,
            last_error: row.get(14)?, mailboxes: json(row, 15)?,
        }))
    }

    fn list_messages(&self) -> Result<Vec<MessageReadModel>, StorageError> {
        let headers = if self.schema_version()? >= 12 {
            "mail_headers_json"
        } else {
            "'{}'"
        };
        collect(&self.connection, &format!("SELECT id, account_id, mailbox, mailbox_role, uid, message_id, from_json, to_json, subject, preview, text_body, html_body, received_at, unread, flagged, has_attachments, attachments_json, labels_json, snoozed_until, {headers} FROM messages ORDER BY received_at DESC"), |row| Ok(MessageReadModel {
            headers: json(row, 19)?,
            id: row.get(0)?, account_id: row.get(1)?, mailbox: row.get(2)?, mailbox_role: row.get(3)?,
            uid: row.get(4)?, message_id: row.get(5)?, from: json(row, 6)?, to: json(row, 7)?,
            subject: row.get(8)?, preview: row.get(9)?, text: row.get(10)?, html: row.get(11)?,
            date: row.get(12)?, unread: boolean(row, 13)?, flagged: boolean(row, 14)?,
            has_attachments: boolean(row, 15)?, attachments: normalized_attachments(json(row, 16)?), labels: json(row, 17)?,
            snoozed_until: row.get(18)?,
        }))
    }

    fn list_drafts(&self) -> Result<Vec<DraftReadModel>, StorageError> {
        let envelope = if self.schema_version()? >= 12 {
            "compose_json"
        } else {
            "'{}'"
        };
        collect(&self.connection, &format!("SELECT id, account_id, to_json, cc_json, subject, text_body, html_body, attachments_json, created_at, updated_at, {envelope} FROM drafts ORDER BY updated_at DESC, id"), |row| Ok(DraftReadModel {
            envelope: json(row, 10)?,
            id: row.get(0)?, account_id: row.get(1)?, to: json(row, 2)?, cc: json(row, 3)?,
            subject: row.get(4)?, text: row.get(5)?, html: row.get(6)?, attachments: json(row, 7)?,
            created_at: row.get(8)?, updated_at: row.get(9)?,
        }))
    }

    fn list_contacts(&self) -> Result<Vec<ContactReadModel>, StorageError> {
        collect(&self.connection, "SELECT user_id, address, name, message_count, last_contact_at, logo_key, logo_content_type, logo_source_url, logo_fetched_at FROM contacts ORDER BY last_contact_at DESC, message_count DESC, address", |row| Ok(ContactReadModel {
            owner_id: row.get(0)?, address: row.get(1)?, name: row.get(2)?, message_count: row.get(3)?,
            last_contact_at: row.get(4)?, logo_key: row.get(5)?, logo_content_type: row.get(6)?,
            logo_source_url: row.get(7)?, logo_fetched_at: row.get(8)?,
        }))
    }

    fn list_developer_tokens(&self) -> Result<Vec<DeveloperTokenReadModel>, StorageError> {
        let scopes = pairs(
            &self.connection,
            "SELECT token_id, scope FROM developer_token_scopes ORDER BY token_id, scope",
        )?;
        let accounts = pairs(&self.connection, "SELECT token_id, account_id FROM developer_token_accounts ORDER BY token_id, account_id")?;
        collect(&self.connection, "SELECT id, user_id, name, prefix, created_at, expires_at, last_used_at FROM developer_tokens ORDER BY created_at DESC, id", |row| {
            let id: String = row.get(0)?;
            Ok(DeveloperTokenReadModel {
                scopes: values_for(&scopes, &id), account_ids: values_for(&accounts, &id), id,
                owner_id: row.get(1)?, name: row.get(2)?, prefix: row.get(3)?, created_at: row.get(4)?,
                expires_at: row.get(5)?, last_used_at: row.get(6)?,
            })
        })
    }

    fn list_sync_policies(&self) -> Result<Vec<SyncPolicyReadModel>, StorageError> {
        collect(&self.connection, "SELECT account_id, enabled, folder_mode, selected_mailboxes_json, notify_on_error, updated_at FROM sync_policies ORDER BY account_id", |row| Ok(SyncPolicyReadModel {
            account_id: row.get(0)?, enabled: boolean(row, 1)?, folder_mode: row.get(2)?,
            selected_mailboxes: json(row, 3)?, notify_on_error: boolean(row, 4)?, updated_at: row.get(5)?,
        }))
    }

    fn list_mailbox_sync_states(&self) -> Result<Vec<MailboxSyncStateReadModel>, StorageError> {
        collect(&self.connection, "SELECT account_id, mailbox, mailbox_role, uid_validity, last_seen_uid, highest_modseq, last_attempt_at, last_success_at, next_sync_at, consecutive_failures, connection_status, sync_state, last_error_code, last_error_message FROM mailbox_sync_states ORDER BY account_id, mailbox", |row| Ok(MailboxSyncStateReadModel {
            account_id: row.get(0)?, mailbox: row.get(1)?, mailbox_role: row.get(2)?, uid_validity: row.get(3)?,
            last_seen_uid: row.get(4)?, highest_modseq: row.get(5)?, last_attempt_at: row.get(6)?,
            last_success_at: row.get(7)?, next_sync_at: row.get(8)?, consecutive_failures: row.get(9)?,
            connection_status: row.get(10)?, sync_state: row.get(11)?, last_error_code: row.get(12)?,
            last_error_message: row.get(13)?,
        }))
    }

    fn list_sync_jobs(&self) -> Result<Vec<SyncJobReadModel>, StorageError> {
        collect(&self.connection, "SELECT id, account_id, mailbox, mailbox_role, reason, status, priority, not_before, locked_by, locked_until, attempts, created_at, started_at, finished_at, synced_count, new_count, updated_count, deleted_count, error_code, error_message, rerun_requested FROM sync_jobs ORDER BY created_at, id", |row| Ok(SyncJobReadModel {
            id: row.get(0)?, account_id: row.get(1)?, mailbox: row.get(2)?, mailbox_role: row.get(3)?,
            reason: row.get(4)?, status: row.get(5)?, priority: row.get(6)?, not_before: row.get(7)?,
            locked_by: row.get(8)?, locked_until: row.get(9)?, attempts: row.get(10)?, created_at: row.get(11)?,
            started_at: row.get(12)?, finished_at: row.get(13)?, synced_count: row.get(14)?,
            new_count: row.get(15)?, updated_count: row.get(16)?, deleted_count: row.get(17)?,
            error_code: row.get(18)?, error_message: row.get(19)?, rerun_requested: boolean(row, 20)?,
        }))
    }

    pub fn compatibility_digests(&self) -> Result<BTreeMap<String, String>, StorageError> {
        let snapshot = self.read_snapshot()?;
        let values = [
            ("accounts", serde_json::to_value(snapshot.accounts)),
            ("messages", serde_json::to_value(snapshot.messages)),
            ("drafts", serde_json::to_value(snapshot.drafts)),
            ("contacts", serde_json::to_value(snapshot.contacts)),
            (
                "developerTokens",
                serde_json::to_value(snapshot.developer_tokens),
            ),
        ];
        let mut output = BTreeMap::new();
        for (name, value) in values {
            let value = value.map_err(|error| {
                StorageError::Integrity(format!("无法序列化 {name} 兼容视图：{error}"))
            })?;
            let canonical = canonical_json(value);
            let digest = Sha256::digest(serde_json::to_vec(&canonical).map_err(|error| {
                StorageError::Integrity(format!("无法编码 {name} 兼容视图：{error}"))
            })?);
            output.insert(name.to_string(), format!("{digest:x}"));
        }
        Ok(output)
    }

    pub fn credential_compatibility_summary(
        &self,
        key: &MasterKey,
    ) -> Result<CredentialCompatibilitySummary, StorageError> {
        let payloads: Vec<String> = self
            .connection
            .prepare("SELECT encrypted_secret FROM accounts ORDER BY id")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let mut field_counts = BTreeMap::new();
        let mut decrypted_count = 0_u64;
        for payload in &payloads {
            let value: Value = key.decrypt_json(payload).map_err(|_| {
                StorageError::Integrity("账户加密凭据无法使用当前主密钥解密".into())
            })?;
            let fields = value
                .as_object()
                .ok_or_else(|| StorageError::Integrity("账户加密凭据不是 JSON 对象".into()))?;
            for field in fields.keys() {
                *field_counts.entry(field.clone()).or_insert(0) += 1;
            }
            decrypted_count += 1;
        }
        let translation_payloads: Vec<String> = self
            .connection
            .prepare(
                "SELECT encrypted_credential FROM translation_provider_profiles
                 WHERE encrypted_credential IS NOT NULL ORDER BY id",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let mut translation_decrypted_count = 0_u64;
        for payload in &translation_payloads {
            let value: Value = key.decrypt_json(payload).map_err(|_| {
                StorageError::Integrity("翻译服务加密凭据无法使用当前主密钥解密".into())
            })?;
            let fields = value
                .as_object()
                .ok_or_else(|| StorageError::Integrity("翻译服务加密凭据不是 JSON 对象".into()))?;
            if !fields.get("kind").is_some_and(Value::is_string)
                || !fields.get("secret").is_some_and(Value::is_string)
            {
                return Err(StorageError::Integrity(
                    "翻译服务加密凭据缺少必要字段".into(),
                ));
            }
            translation_decrypted_count += 1;
        }
        Ok(CredentialCompatibilitySummary {
            account_count: payloads.len() as u64,
            decrypted_count,
            field_counts,
            translation_credential_count: translation_payloads.len() as u64,
            translation_decrypted_count,
        })
    }
}

impl ReadOnlyRepository for SqliteReadOnlyStore {
    type Error = StorageError;

    fn inventory(&self) -> Result<DatabaseInventory, Self::Error> {
        let schema_version = self.ensure_supported_schema()?;
        let quick: Vec<String> = self
            .connection
            .prepare("PRAGMA quick_check")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        if quick.as_slice() != ["ok"] {
            return Err(StorageError::Integrity(quick.join("; ")));
        }
        let foreign_key_violations = self
            .connection
            .prepare("PRAGMA foreign_key_check")?
            .query([])?
            .mapped(|_| Ok(()))
            .count();
        if foreign_key_violations != 0 {
            return Err(StorageError::Integrity(format!(
                "foreign_key_check 发现 {foreign_key_violations} 项异常"
            )));
        }
        let names: Vec<String> = self.connection.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
            .query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?;
        let mut table_counts = BTreeMap::new();
        for name in names.into_iter().filter(|name| {
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        }) {
            let count: u64 = self.connection.query_row(
                &format!("SELECT count(*) FROM \"{name}\""),
                [],
                |row| row.get(0),
            )?;
            table_counts.insert(name, count);
        }
        Ok(DatabaseInventory {
            schema_version,
            quick_check: true,
            foreign_keys_verified: true,
            table_counts,
        })
    }

    fn read_snapshot(&self) -> Result<ReadOnlySnapshot, Self::Error> {
        Ok(ReadOnlySnapshot {
            accounts: self.list_accounts()?,
            messages: self.list_messages()?,
            drafts: self.list_drafts()?,
            contacts: self.list_contacts()?,
            developer_tokens: self.list_developer_tokens()?,
            sync_policies: self.list_sync_policies()?,
            mailbox_sync_states: self.list_mailbox_sync_states()?,
            sync_jobs: self.list_sync_jobs()?,
        })
    }
}

fn collect<T, F>(connection: &Connection, sql: &str, map: F) -> Result<Vec<T>, StorageError>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    Ok(connection
        .prepare(sql)?
        .query_map([], map)?
        .collect::<Result<_, _>>()?)
}

fn json<T: DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let raw: String = row.get(index)?;
    serde_json::from_str(&raw).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
    })
}

fn optional_json(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<Value>> {
    let raw: Option<String> = row.get(index)?;
    raw.map(|value| {
        serde_json::from_str(&value).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
        })
    })
    .transpose()
}

fn boolean(row: &Row<'_>, index: usize) -> rusqlite::Result<bool> {
    match row.get::<_, i64>(index)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Integer,
            format!("布尔字段只能是 0 或 1，实际为 {value}").into(),
        )),
    }
}

fn pairs(connection: &Connection, sql: &str) -> Result<Vec<(String, String)>, StorageError> {
    collect(connection, sql, |row| Ok((row.get(0)?, row.get(1)?)))
}

fn values_for(values: &[(String, String)], key: &str) -> Vec<String> {
    values
        .iter()
        .filter(|(candidate, _)| candidate == key)
        .map(|(_, value)| value.clone())
        .collect()
}

fn normalized_attachments(mut value: Value) -> Value {
    if let Value::Array(attachments) = &mut value {
        for (index, attachment) in attachments.iter_mut().enumerate() {
            if let Value::Object(fields) = attachment {
                fields.entry("index").or_insert_with(|| Value::from(index));
            }
        }
    }
    value
}

fn canonical_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonical_json).collect()),
        Value::Object(values) => {
            let sorted = values
                .into_iter()
                .map(|(key, value)| (key, canonical_json(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests;
