use chrono::{SecondsFormat, Utc};
use rusqlite::OptionalExtension;

use crate::{AuthStoreError, SqliteAuthStore, SqliteReadOnlyStore, StorageError};

#[derive(Clone, PartialEq, Eq)]
pub struct AppleHmeSessionRecord {
    pub account_id: String,
    pub user_id: String,
    pub encrypted_session: String,
    pub updated_at: String,
}

impl SqliteReadOnlyStore {
    pub fn apple_hme_sessions(&self) -> Result<Vec<AppleHmeSessionRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT account_id, user_id, encrypted_session, updated_at
               FROM apple_hme_sessions
              ORDER BY user_id, account_id",
        )?;
        let records = statement
            .query_map([], |row| {
                Ok(AppleHmeSessionRecord {
                    account_id: row.get(0)?,
                    user_id: row.get(1)?,
                    encrypted_session: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }
}

impl SqliteAuthStore {
    pub fn apple_hme_sessions(&self) -> Result<Vec<AppleHmeSessionRecord>, AuthStoreError> {
        let mut statement = self.connection.prepare(
            "SELECT account_id, user_id, encrypted_session, updated_at
               FROM apple_hme_sessions
              ORDER BY user_id, account_id",
        )?;
        let records = statement
            .query_map([], |row| {
                Ok(AppleHmeSessionRecord {
                    account_id: row.get(0)?,
                    user_id: row.get(1)?,
                    encrypted_session: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn apple_hme_session(
        &self,
        user_id: &str,
        account_id: &str,
    ) -> Result<Option<AppleHmeSessionRecord>, AuthStoreError> {
        self.connection
            .query_row(
                "SELECT account_id, user_id, encrypted_session, updated_at
                   FROM apple_hme_sessions
                  WHERE account_id=?1 AND user_id=?2",
                (account_id, user_id),
                |row| {
                    Ok(AppleHmeSessionRecord {
                        account_id: row.get(0)?,
                        user_id: row.get(1)?,
                        encrypted_session: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(AuthStoreError::from)
    }

    pub fn upsert_apple_hme_session(
        &mut self,
        user_id: &str,
        account_id: &str,
        encrypted_session: &str,
    ) -> Result<AppleHmeSessionRecord, AuthStoreError> {
        if encrypted_session.is_empty() || encrypted_session.len() > 2 * 1024 * 1024 {
            return Err(AuthStoreError::InvalidContentData);
        }
        let owned: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND user_id=?2)",
            (account_id, user_id),
            |row| row.get(0),
        )?;
        if !owned {
            return Err(AuthStoreError::AccountNotOwned);
        }
        let updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let changed = self.connection.execute(
            "INSERT INTO apple_hme_sessions(account_id,user_id,encrypted_session,updated_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(account_id) DO UPDATE SET
               user_id=excluded.user_id,
               encrypted_session=excluded.encrypted_session,
               updated_at=excluded.updated_at
             WHERE apple_hme_sessions.user_id=excluded.user_id",
            (account_id, user_id, encrypted_session, &updated_at),
        )?;
        if changed != 1 {
            return Err(AuthStoreError::AccountOwnershipViolation);
        }
        Ok(AppleHmeSessionRecord {
            account_id: account_id.to_string(),
            user_id: user_id.to_string(),
            encrypted_session: encrypted_session.to_string(),
            updated_at,
        })
    }

    pub fn delete_apple_hme_session(
        &mut self,
        user_id: &str,
        account_id: &str,
    ) -> Result<bool, AuthStoreError> {
        Ok(self.connection.execute(
            "DELETE FROM apple_hme_sessions WHERE account_id=?1 AND user_id=?2",
            (account_id, user_id),
        )? == 1)
    }
}
