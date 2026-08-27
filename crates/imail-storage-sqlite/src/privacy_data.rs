use imail_core::PrivacyRepository;
use imail_protocol::ClearMailDataResult;
use rusqlite::params;

use crate::{AuthStoreError, SqliteAuthStore};

impl PrivacyRepository for SqliteAuthStore {
    type Error = AuthStoreError;

    fn clear_user_mail_data(&mut self, user_id: &str) -> Result<ClearMailDataResult, Self::Error> {
        let user_exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM app_users WHERE id=?1)",
            [user_id],
            |row| row.get(0),
        )?;
        if !user_exists {
            return Err(AuthStoreError::UserNotFound);
        }
        let transaction = self.connection.transaction()?;
        let account_count = transaction.query_row(
            "SELECT count(*) FROM accounts WHERE user_id=?1",
            [user_id],
            |row| row.get::<_, u64>(0),
        )?;
        transaction.execute(
            "DELETE FROM developer_token_accounts WHERE token_id IN
               (SELECT id FROM developer_tokens WHERE user_id=?1)
             OR account_id IN (SELECT id FROM accounts WHERE user_id=?1)",
            [user_id],
        )?;
        transaction.execute(
            "DELETE FROM developer_token_scopes WHERE token_id IN
             (SELECT id FROM developer_tokens WHERE user_id=?1)",
            [user_id],
        )?;
        transaction.execute("DELETE FROM developer_tokens WHERE user_id=?1", [user_id])?;
        transaction.execute(
            "DELETE FROM translation_provider_profiles WHERE user_id=?1",
            [user_id],
        )?;
        for key in ["translation_preferences_v1", "translation_environment_v1"] {
            transaction.execute(
                "DELETE FROM metadata WHERE key=?1",
                [format!("user:{user_id}:{key}")],
            )?;
        }
        for statement in [
            "DELETE FROM messages WHERE account_id IN (SELECT id FROM accounts WHERE user_id=?1)",
            "DELETE FROM drafts WHERE account_id IN (SELECT id FROM accounts WHERE user_id=?1)",
            "DELETE FROM sync_policies WHERE account_id IN (SELECT id FROM accounts WHERE user_id=?1)",
            "DELETE FROM mailbox_sync_states WHERE account_id IN (SELECT id FROM accounts WHERE user_id=?1)",
            "DELETE FROM sync_jobs WHERE account_id IN (SELECT id FROM accounts WHERE user_id=?1)",
        ] {
            transaction.execute(statement, params![user_id])?;
        }
        transaction.execute("DELETE FROM accounts WHERE user_id=?1", [user_id])?;
        transaction.execute("DELETE FROM contacts WHERE user_id=?1", [user_id])?;
        transaction.execute(
            "DELETE FROM logo_fetch_attempts WHERE user_id=?1",
            [user_id],
        )?;
        transaction.commit()?;
        Ok(ClearMailDataResult { account_count })
    }
}
