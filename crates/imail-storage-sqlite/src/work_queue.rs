use imail_protocol::{MailWorkItemReadModel, MailWorkStatus};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

use crate::{AuthStoreError, SqliteAuthStore};

impl SqliteAuthStore {
    pub fn list_mail_work_items(
        &self,
        owner: &str,
        status: Option<MailWorkStatus>,
    ) -> Result<Vec<MailWorkItemReadModel>, AuthStoreError> {
        let status = status.map(MailWorkStatus::as_str);
        let mut statement = self.connection.prepare(
            "SELECT id,message_id,account_id,status,due_at,note,draft_id,created_at,updated_at
             FROM mail_work_items
             WHERE user_id=?1 AND (?2 IS NULL OR status=?2)
             ORDER BY due_at IS NULL,due_at,updated_at DESC,id",
        )?;
        let rows = statement
            .query_map(params![owner, status], mail_work_item_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(decode_mail_work_item).collect()
    }

    pub fn mail_work_item(
        &self,
        owner: &str,
        message_id: &str,
    ) -> Result<Option<MailWorkItemReadModel>, AuthStoreError> {
        self.connection
            .query_row(
                "SELECT id,message_id,account_id,status,due_at,note,draft_id,created_at,updated_at
                 FROM mail_work_items WHERE user_id=?1 AND message_id=?2",
                params![owner, message_id],
                mail_work_item_row,
            )
            .optional()?
            .map(decode_mail_work_item)
            .transpose()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_mail_work_item(
        &mut self,
        owner: &str,
        id: &str,
        message_id: &str,
        status: MailWorkStatus,
        due_at: Option<&str>,
        note: &str,
        draft_id: Option<&str>,
        now: &str,
    ) -> Result<MailWorkItemReadModel, AuthStoreError> {
        if note.encode_utf16().count() > 4_000 {
            return Err(AuthStoreError::InvalidContentData);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account_id = tx
            .query_row(
                "SELECT m.account_id FROM messages m JOIN accounts a ON a.id=m.account_id
                 WHERE m.id=?1 AND a.user_id=?2",
                params![message_id, owner],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(AuthStoreError::ContentOwnershipViolation)?;
        if let Some(draft_id) = draft_id {
            let draft_owned = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM drafts d JOIN accounts a ON a.id=d.account_id
                 WHERE d.id=?1 AND a.user_id=?2)",
                params![draft_id, owner],
                |row| row.get::<_, bool>(0),
            )?;
            if !draft_owned {
                return Err(AuthStoreError::ContentOwnershipViolation);
            }
        }
        tx.execute(
            "INSERT INTO mail_work_items(id,user_id,message_id,account_id,status,due_at,note,draft_id,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)
             ON CONFLICT(user_id,message_id) DO UPDATE SET
               status=excluded.status,due_at=excluded.due_at,note=excluded.note,
               draft_id=coalesce(excluded.draft_id,mail_work_items.draft_id),updated_at=excluded.updated_at",
            params![id, owner, message_id, account_id, status.as_str(), due_at, note, draft_id, now],
        )?;
        tx.commit()?;
        self.mail_work_item(owner, message_id)?
            .ok_or(AuthStoreError::InvalidContentData)
    }

    pub fn complete_mail_work_item(
        &mut self,
        owner: &str,
        message_id: &str,
    ) -> Result<bool, AuthStoreError> {
        Ok(self.connection.execute(
            "DELETE FROM mail_work_items WHERE user_id=?1 AND message_id=?2",
            params![owner, message_id],
        )? == 1)
    }
}

type RawMailWorkItem = (
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
);

fn mail_work_item_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawMailWorkItem> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

fn decode_mail_work_item(row: RawMailWorkItem) -> Result<MailWorkItemReadModel, AuthStoreError> {
    let status = match row.3.as_str() {
        "needsReply" => MailWorkStatus::NeedsReply,
        "needsReview" => MailWorkStatus::NeedsReview,
        "followUp" => MailWorkStatus::FollowUp,
        "waiting" => MailWorkStatus::Waiting,
        _ => return Err(AuthStoreError::InvalidContentData),
    };
    Ok(MailWorkItemReadModel {
        id: row.0,
        message_id: row.1,
        account_id: row.2,
        status,
        due_at: row.4,
        note: row.5,
        draft_id: row.6,
        created_at: row.7,
        updated_at: row.8,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::Connection;
    use uuid::Uuid;

    use super::*;

    fn database() -> (std::path::PathBuf, SqliteAuthStore) {
        let root = std::env::temp_dir().join(format!("imail-work-queue-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("imail.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch(&format!(
            "CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL) STRICT;
             INSERT INTO metadata VALUES('schema_version','{}');
             CREATE TABLE accounts(id TEXT PRIMARY KEY,user_id TEXT NOT NULL) STRICT;
             CREATE TABLE messages(id TEXT PRIMARY KEY,account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE) STRICT;
             CREATE TABLE drafts(id TEXT PRIMARY KEY,account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE) STRICT;
             INSERT INTO accounts VALUES('account-1','owner-1'),('account-2','owner-2');
             INSERT INTO messages VALUES('message-1','account-1'),('message-2','account-2');",
            imail_protocol::CURRENT_SCHEMA_VERSION
        )).unwrap();
        connection
            .execute_batch(include_str!("../sql/migration-v17-mail-work-items.sql"))
            .unwrap();
        drop(connection);
        (root, SqliteAuthStore::open_database(&database).unwrap())
    }

    #[test]
    fn upserts_lists_and_completes_owner_scoped_work_items() {
        let (root, mut store) = database();
        let item = store
            .set_mail_work_item(
                "owner-1",
                "item-1",
                "message-1",
                MailWorkStatus::NeedsReply,
                Some("2026-09-02T08:00:00Z"),
                "Respond",
                None,
                "2026-09-01T08:00:00Z",
            )
            .unwrap();
        assert_eq!(item.status, MailWorkStatus::NeedsReply);
        assert!(store
            .list_mail_work_items("owner-2", None)
            .unwrap()
            .is_empty());
        let updated = store
            .set_mail_work_item(
                "owner-1",
                "ignored-new-id",
                "message-1",
                MailWorkStatus::FollowUp,
                None,
                "Follow up",
                None,
                "2026-09-01T09:00:00Z",
            )
            .unwrap();
        assert_eq!(updated.id, "item-1");
        assert_eq!(updated.status, MailWorkStatus::FollowUp);
        assert!(store
            .set_mail_work_item(
                "owner-1",
                "bad",
                "message-2",
                MailWorkStatus::NeedsReply,
                None,
                "",
                None,
                "2026-09-01T09:00:00Z",
            )
            .is_err());
        assert!(store
            .complete_mail_work_item("owner-1", "message-1")
            .unwrap());
        assert!(store
            .list_mail_work_items("owner-1", None)
            .unwrap()
            .is_empty());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
