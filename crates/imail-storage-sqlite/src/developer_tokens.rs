use std::collections::BTreeSet;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{Duration, SecondsFormat, Utc};
use imail_core::{DeveloperTokenRepository, IssuedDeveloperToken};
use imail_protocol::DeveloperTokenReadModel;
use imail_security::sha256_hex;
use rand::{rngs::OsRng, RngCore};
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::{AuthStoreError, SqliteAuthStore};

const MIN_TTL_SECONDS: u64 = 300;
const MAX_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
const ALLOWED_SCOPES: [&str; 4] = [
    "messages:read",
    "messages:send",
    "accounts:read",
    "mcp:full",
];

impl DeveloperTokenRepository for SqliteAuthStore {
    type Error = AuthStoreError;

    fn issue_developer_token(
        &mut self,
        user_id: &str,
        name: &str,
        scopes: &[String],
        requested_account_ids: &[String],
        ttl_seconds: u64,
    ) -> Result<IssuedDeveloperToken, Self::Error> {
        if name.is_empty() || name.chars().count() > 80 {
            return Err(AuthStoreError::InvalidTokenName);
        }
        if !(MIN_TTL_SECONDS..=MAX_TTL_SECONDS).contains(&ttl_seconds) {
            return Err(AuthStoreError::InvalidTokenTtl);
        }
        let normalized_scopes = scopes.iter().cloned().collect::<BTreeSet<_>>();
        if normalized_scopes.is_empty()
            || normalized_scopes.len() != scopes.len()
            || normalized_scopes
                .iter()
                .any(|scope| !ALLOWED_SCOPES.contains(&scope.as_str()))
        {
            return Err(AuthStoreError::InvalidTokenScope);
        }
        let is_mcp = normalized_scopes.contains("mcp:full");
        if is_mcp && normalized_scopes.len() != 1 {
            return Err(AuthStoreError::InvalidTokenScope);
        }

        let user_exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM app_users WHERE id = ?1)",
            [user_id],
            |row| row.get(0),
        )?;
        if !user_exists {
            return Err(AuthStoreError::UserNotFound);
        }
        let owned_accounts = {
            let mut statement = self
                .connection
                .prepare("SELECT id FROM accounts WHERE user_id = ?1 ORDER BY id")?;
            let accounts = statement
                .query_map([user_id], |row| row.get::<_, String>(0))?
                .collect::<Result<BTreeSet<_>, _>>()?;
            accounts
        };
        let requested_accounts = requested_account_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if requested_accounts.len() != requested_account_ids.len()
            || (!is_mcp && requested_accounts.is_empty())
            || !requested_accounts.is_subset(&owned_accounts)
        {
            return Err(AuthStoreError::InvalidTokenAccounts);
        }
        let account_ids = if is_mcp {
            owned_accounts.into_iter().collect::<Vec<_>>()
        } else {
            requested_accounts.into_iter().collect::<Vec<_>>()
        };

        let mut random = [0_u8; 30];
        OsRng.fill_bytes(&mut random);
        let prefix = if is_mcp { "imail_mcp_" } else { "imail_" };
        let raw = format!("{prefix}{}", URL_SAFE_NO_PAD.encode(random));
        let created = Utc::now();
        let token = DeveloperTokenReadModel {
            id: Uuid::new_v4().to_string(),
            owner_id: user_id.to_string(),
            name: name.to_string(),
            prefix: raw.chars().take(12).collect(),
            scopes: normalized_scopes.into_iter().collect(),
            account_ids,
            created_at: created.to_rfc3339_opts(SecondsFormat::Millis, true),
            expires_at: (created + Duration::seconds(ttl_seconds as i64))
                .to_rfc3339_opts(SecondsFormat::Millis, true),
            last_used_at: None,
        };
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO developer_tokens
             (id, name, token_hash, prefix, created_at, expires_at, last_used_at, user_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7)",
            (
                &token.id,
                &token.name,
                sha256_hex(&raw),
                &token.prefix,
                &token.created_at,
                &token.expires_at,
                &token.owner_id,
            ),
        )?;
        for scope in &token.scopes {
            transaction.execute(
                "INSERT INTO developer_token_scopes (token_id, scope) VALUES (?1, ?2)",
                (&token.id, scope),
            )?;
        }
        for account_id in &token.account_ids {
            transaction.execute(
                "INSERT INTO developer_token_accounts (token_id, account_id) VALUES (?1, ?2)",
                (&token.id, account_id),
            )?;
        }
        transaction.commit()?;
        Ok(IssuedDeveloperToken { token, raw })
    }

    fn authenticate_developer_token(
        &mut self,
        raw: &str,
        required_scope: &str,
    ) -> Result<Option<DeveloperTokenReadModel>, Self::Error> {
        if !raw.starts_with("imail_") || !ALLOWED_SCOPES.contains(&required_scope) {
            return Ok(None);
        }
        let token_id: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM developer_tokens
                 WHERE token_hash = ?1 AND expires_at > ?2
                   AND EXISTS (SELECT 1 FROM developer_token_scopes
                               WHERE token_id = developer_tokens.id AND scope = ?3)",
                (sha256_hex(raw), now(), required_scope),
                |row| row.get(0),
            )
            .optional()?;
        let Some(token_id) = token_id else {
            return Ok(None);
        };
        let used_at = now();
        self.connection.execute(
            "UPDATE developer_tokens SET last_used_at = ?1 WHERE id = ?2",
            (&used_at, &token_id),
        )?;
        self.token_by_id(&token_id)
    }

    fn list_developer_tokens(
        &self,
        user_id: &str,
    ) -> Result<Vec<DeveloperTokenReadModel>, Self::Error> {
        let mut statement = self.connection.prepare(
            "SELECT id FROM developer_tokens WHERE user_id = ?1 ORDER BY created_at DESC, id",
        )?;
        let ids = statement
            .query_map([user_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut tokens = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(token) = self.token_by_id(&id)? {
                tokens.push(token);
            }
        }
        Ok(tokens)
    }

    fn revoke_developer_token(
        &mut self,
        user_id: &str,
        token_id: &str,
    ) -> Result<bool, Self::Error> {
        let transaction = self.connection.transaction()?;
        let owned: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM developer_tokens WHERE id = ?1 AND user_id = ?2)",
            (token_id, user_id),
            |row| row.get(0),
        )?;
        if owned {
            transaction.execute(
                "DELETE FROM developer_token_accounts WHERE token_id = ?1",
                [token_id],
            )?;
            transaction.execute(
                "DELETE FROM developer_token_scopes WHERE token_id = ?1",
                [token_id],
            )?;
            transaction.execute(
                "DELETE FROM developer_tokens WHERE id = ?1 AND user_id = ?2",
                (token_id, user_id),
            )?;
        }
        transaction.commit()?;
        Ok(owned)
    }
}

impl SqliteAuthStore {
    fn token_by_id(
        &self,
        token_id: &str,
    ) -> Result<Option<DeveloperTokenReadModel>, AuthStoreError> {
        let token = self
            .connection
            .query_row(
                "SELECT id, user_id, name, prefix, created_at, expires_at, last_used_at
                 FROM developer_tokens WHERE id = ?1",
                [token_id],
                |row| {
                    Ok(DeveloperTokenReadModel {
                        id: row.get(0)?,
                        owner_id: row.get(1)?,
                        name: row.get(2)?,
                        prefix: row.get(3)?,
                        scopes: Vec::new(),
                        account_ids: Vec::new(),
                        created_at: row.get(4)?,
                        expires_at: row.get(5)?,
                        last_used_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        let Some(mut token) = token else {
            return Ok(None);
        };
        token.scopes = values(
            &self.connection,
            "SELECT scope FROM developer_token_scopes WHERE token_id = ?1 ORDER BY scope",
            token_id,
        )?;
        token.account_ids = values(
            &self.connection,
            "SELECT account_id FROM developer_token_accounts WHERE token_id = ?1 ORDER BY account_id",
            token_id,
        )?;
        Ok(Some(token))
    }
}

fn values(
    connection: &rusqlite::Connection,
    sql: &str,
    token_id: &str,
) -> Result<Vec<String>, rusqlite::Error> {
    let mut statement = connection.prepare(sql)?;
    let output = statement
        .query_map([token_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(output)
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
