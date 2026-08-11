use imail_core::{AccountRecord, AccountRepository};
use imail_security::MasterKey;
use rusqlite::{params, OptionalExtension};

use crate::{AuthStoreError, SqliteAuthStore};

impl SqliteAuthStore {
    pub fn reencrypt_account_credential(
        &mut self,
        account_id: &str,
        master_key: &MasterKey,
    ) -> Result<bool, AuthStoreError> {
        let encrypted: Option<String> = self
            .connection
            .query_row(
                "SELECT encrypted_secret FROM accounts WHERE id=?1",
                [account_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(encrypted) = encrypted else {
            return Ok(false);
        };
        let semantic: serde_json::Value = master_key.decrypt_json(&encrypted)?;
        let replacement = master_key.encrypt_json(&semantic)?;
        if replacement == encrypted
            || master_key.decrypt_json::<serde_json::Value>(&replacement)? != semantic
        {
            return Err(AuthStoreError::Security(
                imail_security::SecurityError::AuthenticationFailed,
            ));
        }
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE accounts SET encrypted_secret=?1 WHERE id=?2 AND encrypted_secret=?3",
            (&replacement, account_id, &encrypted),
        )?;
        if updated != 1 {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }
}

impl AccountRepository for SqliteAuthStore {
    type Error = AuthStoreError;

    fn account(
        &self,
        user_id: &str,
        account_id: &str,
    ) -> Result<Option<AccountRecord>, Self::Error> {
        let row = self
            .connection
            .query_row(
                "SELECT id, user_id, provider, email, display_name, group_name, group_icon,
                        color, settings_json, proxy_json, encrypted_secret, auth_method,
                        created_at, last_sync_at, status, last_error, mailboxes_json
                 FROM accounts WHERE id = ?1 AND user_id = ?2",
                (account_id, user_id),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, Option<String>>(11)?,
                        row.get::<_, String>(12)?,
                        row.get::<_, Option<String>>(13)?,
                        row.get::<_, String>(14)?,
                        row.get::<_, Option<String>>(15)?,
                        row.get::<_, String>(16)?,
                    ))
                },
            )
            .optional()?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(AccountRecord {
            id: row.0,
            owner_id: row.1,
            provider: row.2,
            email: row.3,
            display_name: row.4,
            group: row.5,
            group_icon: row.6,
            color: row.7,
            settings: serde_json::from_str(&row.8)?,
            proxy: row
                .9
                .map(|value| serde_json::from_str(&value))
                .transpose()?,
            encrypted_secret: row.10,
            auth_method: row.11,
            created_at: row.12,
            last_sync_at: row.13,
            status: row.14,
            last_error: row.15,
            mailboxes: serde_json::from_str(&row.16)?,
        }))
    }

    fn list_accounts(&self, user_id: &str) -> Result<Vec<AccountRecord>, Self::Error> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM accounts WHERE user_id = ?1 ORDER BY created_at, id")?;
        let ids = statement
            .query_map([user_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut accounts = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(account) = self.account(user_id, &id)? {
                accounts.push(account);
            }
        }
        Ok(accounts)
    }

    fn insert_account_if_email_available(
        &mut self,
        account: &AccountRecord,
    ) -> Result<bool, Self::Error> {
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let duplicate: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE user_id=?1 AND lower(email)=lower(?2))",
            params![account.owner_id, account.email],
            |row| row.get(0),
        )?;
        if duplicate {
            transaction.commit()?;
            return Ok(false);
        }
        transaction.execute(
            "INSERT INTO accounts
             (id,provider,email,display_name,group_name,group_icon,color,settings_json,proxy_json,
              encrypted_secret,auth_method,created_at,last_sync_at,status,last_error,mailboxes_json,user_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![
                account.id,
                account.provider,
                account.email,
                account.display_name,
                account.group,
                account.group_icon,
                account.color,
                serde_json::to_string(&account.settings)?,
                account
                    .proxy
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?,
                account.encrypted_secret,
                account.auth_method,
                account.created_at,
                account.last_sync_at,
                account.status,
                account.last_error,
                serde_json::to_string(&account.mailboxes)?,
                account.owner_id,
            ],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    fn upsert_account(&mut self, account: &AccountRecord) -> Result<(), Self::Error> {
        let user_exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM app_users WHERE id = ?1)",
            [&account.owner_id],
            |row| row.get(0),
        )?;
        if !user_exists {
            return Err(AuthStoreError::UserNotFound);
        }
        let existing_owner: Option<String> = self
            .connection
            .query_row(
                "SELECT user_id FROM accounts WHERE id = ?1",
                [&account.id],
                |row| row.get(0),
            )
            .optional()?;
        if existing_owner
            .as_deref()
            .is_some_and(|owner| owner != account.owner_id)
        {
            return Err(AuthStoreError::AccountOwnershipViolation);
        }
        let duplicate: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts
                           WHERE user_id = ?1 AND email = ?2 COLLATE NOCASE AND id <> ?3)",
            (&account.owner_id, &account.email, &account.id),
            |row| row.get(0),
        )?;
        if duplicate {
            return Err(AuthStoreError::DuplicateAccount);
        }
        let settings = serde_json::to_string(&account.settings)?;
        let proxy = account
            .proxy
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let mailboxes = serde_json::to_string(&account.mailboxes)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO accounts
             (id, provider, email, display_name, group_name, group_icon, color,
              settings_json, proxy_json, encrypted_secret, auth_method, created_at,
              last_sync_at, status, last_error, mailboxes_json, user_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(id) DO UPDATE SET
               provider=excluded.provider, email=excluded.email,
               display_name=excluded.display_name, group_name=excluded.group_name,
               group_icon=excluded.group_icon, color=excluded.color,
               settings_json=excluded.settings_json, proxy_json=excluded.proxy_json,
               encrypted_secret=excluded.encrypted_secret, auth_method=excluded.auth_method,
               last_sync_at=excluded.last_sync_at, status=excluded.status,
               last_error=excluded.last_error, mailboxes_json=excluded.mailboxes_json
             WHERE accounts.user_id=excluded.user_id",
            params![
                account.id,
                account.provider,
                account.email.to_lowercase(),
                account.display_name,
                account.group,
                account.group_icon,
                account.color,
                settings,
                proxy,
                account.encrypted_secret,
                account.auth_method,
                account.created_at,
                account.last_sync_at,
                account.status,
                account.last_error,
                mailboxes,
                account.owner_id,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn delete_account(&mut self, user_id: &str, account_id: &str) -> Result<bool, Self::Error> {
        let transaction = self.connection.transaction()?;
        let owned: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND user_id=?2)",
            (account_id, user_id),
            |row| row.get(0),
        )?;
        if !owned {
            transaction.commit()?;
            return Ok(false);
        }
        transaction.execute(
            "DELETE FROM developer_token_accounts WHERE account_id=?1",
            [account_id],
        )?;
        for statement in [
            "DELETE FROM messages WHERE account_id=?1",
            "DELETE FROM drafts WHERE account_id=?1",
            "DELETE FROM sync_policies WHERE account_id=?1",
            "DELETE FROM mailbox_sync_states WHERE account_id=?1",
            "DELETE FROM sync_jobs WHERE account_id=?1",
        ] {
            transaction.execute(statement, [account_id])?;
        }
        let deleted = transaction.execute(
            "DELETE FROM accounts WHERE id = ?1 AND user_id = ?2",
            (account_id, user_id),
        )?;
        transaction.commit()?;
        Ok(deleted == 1)
    }

    fn user_metadata(&self, user_id: &str, key: &str) -> Result<Option<String>, Self::Error> {
        validate_metadata_key(key)?;
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                [metadata_key(user_id, key)],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn set_user_metadata(
        &mut self,
        user_id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), Self::Error> {
        validate_metadata_key(key)?;
        let user_exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM app_users WHERE id = ?1)",
            [user_id],
            |row| row.get(0),
        )?;
        if !user_exists {
            return Err(AuthStoreError::UserNotFound);
        }
        self.connection.execute(
            "INSERT INTO metadata (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            (metadata_key(user_id, key), value),
        )?;
        Ok(())
    }
}

fn validate_metadata_key(key: &str) -> Result<(), AuthStoreError> {
    if key.is_empty()
        || key.len() > 80
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(AuthStoreError::InvalidMetadataKey);
    }
    Ok(())
}

fn metadata_key(user_id: &str, key: &str) -> String {
    format!("user:{user_id}:{key}")
}
