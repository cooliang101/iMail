use std::{collections::BTreeMap, path::Path};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use imail_core::AuthRepository;
use imail_protocol::{AppUserView, RateLimitDecision, CURRENT_SCHEMA_VERSION};
use imail_security::{
    audit_actor_hmac_hex, hash_password, sha256_hex, verify_password, SecurityError,
};
use rand::{rngs::OsRng, RngCore};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use thiserror::Error;
use uuid::Uuid;

const SESSION_TTL_DAYS: i64 = 30;
const AUDIT_RETENTION_DAYS: i64 = 90;
const MAX_AUDIT_EVENTS: i64 = 10_000;
const MAX_RATE_LIMIT_ROWS: i64 = 10_000;
const RATE_LIMIT_PRUNE_ROWS: i64 = 100;

#[derive(Debug, Error)]
pub enum AuthStoreError {
    #[error("找不到 iMail 数据库")]
    MissingDatabase,
    #[error("数据库 schema v{actual} 不允许 Rust 写入，当前要求 v{required}")]
    UnsupportedSchema { actual: u32, required: u32 },
    #[error("iMail 数据库 schema 版本无效")]
    InvalidSchema,
    #[error("应用用户不存在")]
    UserNotFound,
    #[error("开发者 Token 名称无效")]
    InvalidTokenName,
    #[error("开发者 Token scope 无效")]
    InvalidTokenScope,
    #[error("开发者 Token 有效期无效")]
    InvalidTokenTtl,
    #[error("开发者 Token 必须绑定当前用户的邮箱账户")]
    InvalidTokenAccounts,
    #[error("用户 metadata 键无效")]
    InvalidMetadataKey,
    #[error("邮箱账户属于其他用户")]
    AccountOwnershipViolation,
    #[error("当前用户已存在相同邮箱账户")]
    DuplicateAccount,
    #[error("引用的邮箱账户不存在或不属于当前用户")]
    AccountNotOwned,
    #[error("内容记录属于其他用户")]
    ContentOwnershipViolation,
    #[error("内容记录字段组合无效")]
    InvalidContentData,
    #[error("翻译服务配置属于其他用户")]
    TranslationProfileOwnershipViolation,
    #[error("安全审计查询无效")]
    InvalidAuditQuery,
    #[error(transparent)]
    Security(#[from] SecurityError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Chrono(#[from] chrono::ParseError),
}

impl AuthStoreError {
    pub fn is_unique_violation(&self) -> bool {
        matches!(
            self,
            Self::Sqlite(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation
        )
    }
}

pub struct SqliteAuthStore {
    pub(crate) connection: Connection,
    audit_actor_salt: String,
}

impl SqliteAuthStore {
    pub fn open_database(database_path: impl AsRef<Path>) -> Result<Self, AuthStoreError> {
        let database_path = database_path.as_ref();
        if !database_path.is_file() {
            return Err(AuthStoreError::MissingDatabase);
        }
        let connection = Connection::open_with_flags(
            database_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let raw: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AuthStoreError::InvalidSchema,
                other => AuthStoreError::Sqlite(other),
            })?;
        let actual = raw
            .parse::<u32>()
            .map_err(|_| AuthStoreError::InvalidSchema)?;
        if actual != CURRENT_SCHEMA_VERSION {
            return Err(AuthStoreError::UnsupportedSchema {
                actual,
                required: CURRENT_SCHEMA_VERSION,
            });
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_users (
               id TEXT PRIMARY KEY, login TEXT NOT NULL COLLATE NOCASE UNIQUE,
               display_name TEXT NOT NULL, password_hash TEXT NOT NULL, created_at TEXT NOT NULL
             ) STRICT;
             CREATE TABLE IF NOT EXISTS app_sessions (
               id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
               token_hash TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL,
               expires_at TEXT NOT NULL, last_seen_at TEXT NOT NULL
             ) STRICT;
             CREATE INDEX IF NOT EXISTS app_sessions_expiry ON app_sessions(expires_at);
             CREATE TABLE IF NOT EXISTS auth_rate_limits (
               key_hash TEXT PRIMARY KEY, attempt_count INTEGER NOT NULL, reset_at TEXT NOT NULL
             ) STRICT;
             CREATE INDEX IF NOT EXISTS auth_rate_limits_expiry ON auth_rate_limits(reset_at);
             CREATE TABLE IF NOT EXISTS security_audit_events (
               id TEXT PRIMARY KEY, event_type TEXT NOT NULL, user_id TEXT,
               actor_hash TEXT NOT NULL, detail_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL
             ) STRICT;
             CREATE INDEX IF NOT EXISTS security_audit_events_created
               ON security_audit_events(created_at DESC);",
        )?;
        let mut salt_bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut salt_bytes);
        connection.execute(
            "INSERT INTO metadata (key, value) VALUES ('security_audit_actor_salt', ?1)
             ON CONFLICT(key) DO NOTHING",
            [hex(&salt_bytes)],
        )?;
        let audit_actor_salt = connection.query_row(
            "SELECT value FROM metadata WHERE key = 'security_audit_actor_salt'",
            [],
            |row| row.get(0),
        )?;
        Ok(Self {
            connection,
            audit_actor_salt,
        })
    }

    pub fn setup_required(&self) -> Result<bool, AuthStoreError> {
        let total: i64 =
            self.connection
                .query_row("SELECT count(*) FROM app_users", [], |row| row.get(0))?;
        Ok(total == 0)
    }

    fn user_by_login(&self, login: &str) -> Result<Option<(AppUserView, String)>, AuthStoreError> {
        Ok(self
            .connection
            .query_row(
                "SELECT id, login, display_name, created_at, password_hash
                 FROM app_users WHERE login = ?1 COLLATE NOCASE",
                [login],
                |row| {
                    Ok((
                        AppUserView {
                            id: row.get(0)?,
                            login: row.get(1)?,
                            display_name: row.get(2)?,
                            created_at: row.get(3)?,
                        },
                        row.get(4)?,
                    ))
                },
            )
            .optional()?)
    }

    pub fn create_user_if_allowed(
        &mut self,
        login: &str,
        display_name: &str,
        password: &str,
        registration_open: bool,
    ) -> Result<Option<AppUserView>, AuthStoreError> {
        if !registration_open && !self.setup_required()? {
            return Ok(None);
        }
        let user = AppUserView {
            id: Uuid::new_v4().to_string(),
            login: login.to_lowercase(),
            display_name: display_name.to_string(),
            created_at: now(),
        };
        let password_hash = hash_password(password)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous_total: i64 =
            transaction.query_row("SELECT count(*) FROM app_users", [], |row| row.get(0))?;
        if previous_total > 0 && !registration_open {
            transaction.commit()?;
            return Ok(None);
        }
        transaction.execute(
            "INSERT INTO app_users (id, login, display_name, password_hash, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                &user.id,
                &user.login,
                &user.display_name,
                &password_hash,
                &user.created_at,
            ),
        )?;
        if previous_total == 0 {
            for table in [
                "accounts",
                "developer_tokens",
                "contacts",
                "logo_fetch_attempts",
            ] {
                transaction.execute(
                    &format!("UPDATE {table} SET user_id = ?1 WHERE user_id = '__legacy__'"),
                    [&user.id],
                )?;
            }
            let preference_key = format!("user:{}:app_preferences_v1", user.id);
            transaction.execute(
                "UPDATE metadata SET key = ?1 WHERE key = 'app_preferences_v1'
                 AND NOT EXISTS (SELECT 1 FROM metadata WHERE key = ?1)",
                [&preference_key],
            )?;
        }
        transaction.commit()?;
        Ok(Some(user))
    }
}

impl AuthRepository for SqliteAuthStore {
    type Error = AuthStoreError;

    fn create_user(
        &mut self,
        login: &str,
        display_name: &str,
        password: &str,
    ) -> Result<AppUserView, Self::Error> {
        self.create_user_if_allowed(login, display_name, password, true)?
            .ok_or(AuthStoreError::UserNotFound)
    }

    fn setup_required(&self) -> Result<bool, Self::Error> {
        SqliteAuthStore::setup_required(self)
    }

    fn create_user_if_allowed(
        &mut self,
        login: &str,
        display_name: &str,
        password: &str,
        registration_open: bool,
    ) -> Result<Option<AppUserView>, Self::Error> {
        SqliteAuthStore::create_user_if_allowed(
            self,
            login,
            display_name,
            password,
            registration_open,
        )
    }

    fn authenticate(
        &self,
        login: &str,
        password: &str,
    ) -> Result<Option<AppUserView>, Self::Error> {
        let Some((user, encoded)) = self.user_by_login(login)? else {
            return Ok(None);
        };
        Ok(verify_password(password, &encoded)?.then_some(user))
    }

    fn verify_user_password(&self, user_id: &str, password: &str) -> Result<bool, Self::Error> {
        let encoded: Option<String> = self
            .connection
            .query_row(
                "SELECT password_hash FROM app_users WHERE id = ?1",
                [user_id],
                |row| row.get(0),
            )
            .optional()?;
        match encoded {
            Some(encoded) => Ok(verify_password(password, &encoded)?),
            None => Ok(false),
        }
    }

    fn create_session(&mut self, user_id: &str) -> Result<String, Self::Error> {
        let mut raw_bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut raw_bytes);
        let raw = URL_SAFE_NO_PAD.encode(raw_bytes);
        let created_at = Utc::now();
        let created_text = created_at.to_rfc3339_opts(SecondsFormat::Millis, true);
        let expires_at = (created_at + Duration::days(SESSION_TTL_DAYS))
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM app_sessions WHERE expires_at <= ?1",
            [&created_text],
        )?;
        transaction.execute(
            "INSERT INTO app_sessions
             (id, user_id, token_hash, created_at, expires_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?4)",
            (
                Uuid::new_v4().to_string(),
                user_id,
                sha256_hex(&raw),
                &created_text,
                expires_at,
            ),
        )?;
        transaction.commit()?;
        Ok(raw)
    }

    fn user_for_session(&mut self, raw_session: &str) -> Result<Option<AppUserView>, Self::Error> {
        let current = now();
        let row: Option<(AppUserView, String)> = self
            .connection
            .query_row(
                "SELECT u.id, u.login, u.display_name, u.created_at, s.id
                 FROM app_sessions s JOIN app_users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1 AND s.expires_at > ?2",
                (sha256_hex(raw_session), &current),
                |row| {
                    Ok((
                        AppUserView {
                            id: row.get(0)?,
                            login: row.get(1)?,
                            display_name: row.get(2)?,
                            created_at: row.get(3)?,
                        },
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        if let Some((user, session_id)) = row {
            self.connection.execute(
                "UPDATE app_sessions SET last_seen_at = ?1 WHERE id = ?2",
                (&current, session_id),
            )?;
            Ok(Some(user))
        } else {
            Ok(None)
        }
    }

    fn delete_session(&mut self, raw_session: &str) -> Result<(), Self::Error> {
        self.connection.execute(
            "DELETE FROM app_sessions WHERE token_hash = ?1",
            [sha256_hex(raw_session)],
        )?;
        Ok(())
    }

    fn consume_attempt(
        &mut self,
        key: &str,
        maximum: u32,
        window_ms: u64,
    ) -> Result<RateLimitDecision, Self::Error> {
        let current = Utc::now();
        let current_text = current.to_rfc3339_opts(SecondsFormat::Millis, true);
        let reset_at = (current + Duration::milliseconds(window_ms.min(i64::MAX as u64) as i64))
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let key_hash = sha256_hex(key);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM auth_rate_limits WHERE reset_at <= ?1",
            [&current_text],
        )?;
        let row: Option<(u32, String)> = transaction
            .query_row(
                "SELECT attempt_count, reset_at FROM auth_rate_limits WHERE key_hash = ?1",
                [&key_hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((attempt_count, existing_reset)) = row {
            if attempt_count >= maximum {
                let reset = DateTime::parse_from_rfc3339(&existing_reset)?.with_timezone(&Utc);
                let remaining_ms = (reset - current).num_milliseconds().max(1) as u64;
                transaction.commit()?;
                return Ok(RateLimitDecision {
                    allowed: false,
                    retry_after: remaining_ms.div_ceil(1_000).max(1),
                });
            }
            transaction.execute(
                "UPDATE auth_rate_limits SET attempt_count = attempt_count + 1 WHERE key_hash = ?1",
                [&key_hash],
            )?;
        } else {
            let total: i64 =
                transaction.query_row("SELECT count(*) FROM auth_rate_limits", [], |row| {
                    row.get(0)
                })?;
            if total >= MAX_RATE_LIMIT_ROWS {
                transaction.execute(
                    "DELETE FROM auth_rate_limits WHERE key_hash IN (
                       SELECT key_hash FROM auth_rate_limits ORDER BY reset_at LIMIT ?1
                     )",
                    [RATE_LIMIT_PRUNE_ROWS],
                )?;
            }
            transaction.execute(
                "INSERT INTO auth_rate_limits (key_hash, attempt_count, reset_at)
                 VALUES (?1, 1, ?2)",
                (&key_hash, &reset_at),
            )?;
        }
        transaction.commit()?;
        Ok(RateLimitDecision {
            allowed: true,
            retry_after: 0,
        })
    }

    fn clear_attempt(&mut self, key: &str) -> Result<(), Self::Error> {
        self.connection.execute(
            "DELETE FROM auth_rate_limits WHERE key_hash = ?1",
            [sha256_hex(key)],
        )?;
        Ok(())
    }

    fn record_security_event(
        &mut self,
        event_type: &str,
        actor: &str,
        user_id: Option<&str>,
        detail: &BTreeMap<String, String>,
    ) -> Result<(), Self::Error> {
        let created = Utc::now();
        let created_at = created.to_rfc3339_opts(SecondsFormat::Millis, true);
        let cutoff = (created - Duration::days(AUDIT_RETENTION_DAYS))
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM security_audit_events WHERE created_at < ?1",
            [&cutoff],
        )?;
        transaction.execute(
            "INSERT INTO security_audit_events
             (id, event_type, user_id, actor_hash, detail_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                Uuid::new_v4().to_string(),
                event_type,
                user_id,
                audit_actor_hmac_hex(
                    &self.audit_actor_salt,
                    if actor.is_empty() { "unknown" } else { actor },
                ),
                serde_json::to_string(detail)?,
                &created_at,
            ),
        )?;
        let total: i64 =
            transaction.query_row("SELECT count(*) FROM security_audit_events", [], |row| {
                row.get(0)
            })?;
        if total > MAX_AUDIT_EVENTS {
            transaction.execute(
                "DELETE FROM security_audit_events WHERE id IN (
                   SELECT id FROM security_audit_events ORDER BY created_at ASC, id ASC LIMIT ?1
                 )",
                [total - MAX_AUDIT_EVENTS],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

impl SqliteAuthStore {
    pub fn security_audit_events(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<imail_protocol::SecurityAuditEventReadModel>, AuthStoreError> {
        if user_id.is_empty() || limit == 0 || limit > 500 {
            return Err(AuthStoreError::InvalidAuditQuery);
        }
        let mut statement = self.connection.prepare(
            "SELECT id, event_type, actor_hash, detail_json, created_at
             FROM security_audit_events
             WHERE user_id=?1 ORDER BY created_at DESC,id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map((user_id, limit as i64), |row| {
            let detail_json = row.get::<_, String>(3)?;
            Ok(imail_protocol::SecurityAuditEventReadModel {
                id: row.get(0)?,
                event_type: row.get(1)?,
                actor_hash: row.get(2)?,
                detail: serde_json::from_str(&detail_json)
                    .unwrap_or_else(|_| serde_json::Value::Object(Default::default())),
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn security_audit_details(
        &self,
        user_id: &str,
        event_type: &str,
        limit: usize,
    ) -> Result<Vec<BTreeMap<String, String>>, AuthStoreError> {
        if user_id.is_empty() || event_type.is_empty() || limit == 0 || limit > 500 {
            return Err(AuthStoreError::InvalidAuditQuery);
        }
        let mut statement = self.connection.prepare(
            "SELECT detail_json FROM security_audit_events
             WHERE user_id=?1 AND event_type=?2 ORDER BY created_at DESC,id DESC LIMIT ?3",
        )?;
        let rows = statement.query_map((user_id, event_type, limit as i64), |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| {
            serde_json::from_str(&row?).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
                .into()
            })
        })
        .collect()
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
