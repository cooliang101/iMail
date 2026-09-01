use chrono::{DateTime, Duration, SecondsFormat, Utc};
use imail_protocol::{OutboxItemReadModel, OutboxStatus, SendMessageInput};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

use crate::{AuthStoreError, SqliteAuthStore};

const LEASE_MINUTES: i64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxWorkItem {
    pub id: String,
    pub owner_id: String,
    pub message: SendMessageInput,
    pub draft_id: Option<String>,
}

impl SqliteAuthStore {
    pub fn schedule_outbox(
        &mut self,
        owner: &str,
        id: &str,
        message: &SendMessageInput,
        draft_id: Option<&str>,
        scheduled_at: &str,
        now: &str,
    ) -> Result<OutboxItemReadModel, AuthStoreError> {
        let owned = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND user_id=?2)",
            params![message.account_id, owner],
            |row| row.get::<_, bool>(0),
        )?;
        if !owned {
            return Err(AuthStoreError::AccountNotOwned);
        }
        self.connection.execute(
            "INSERT INTO outbox_items(id,user_id,account_id,message_json,draft_id,scheduled_at,status,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,'scheduled',?7,?7)",
            params![id, owner, message.account_id, serde_json::to_string(message)?, draft_id, scheduled_at, now],
        )?;
        self.outbox_item(owner, id)?
            .ok_or(AuthStoreError::InvalidContentData)
    }

    pub fn list_outbox(&self, owner: &str) -> Result<Vec<OutboxItemReadModel>, AuthStoreError> {
        let mut statement = self.connection.prepare(
            "SELECT id,account_id,message_json,scheduled_at,status,attempts,last_error_message,created_at,updated_at,sent_at
             FROM outbox_items WHERE user_id=?1
             ORDER BY CASE status WHEN 'sending' THEN 0 WHEN 'scheduled' THEN 1 WHEN 'needsReview' THEN 2 WHEN 'failed' THEN 3 ELSE 4 END,
                      scheduled_at DESC,created_at DESC",
        )?;
        let rows = statement
            .query_map([owner], outbox_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(decode_outbox_row).collect()
    }

    pub fn outbox_item(
        &self,
        owner: &str,
        id: &str,
    ) -> Result<Option<OutboxItemReadModel>, AuthStoreError> {
        self.connection.query_row(
            "SELECT id,account_id,message_json,scheduled_at,status,attempts,last_error_message,created_at,updated_at,sent_at
             FROM outbox_items WHERE user_id=?1 AND id=?2",
            params![owner,id], outbox_row,
        ).optional()?.map(decode_outbox_row).transpose()
    }

    pub fn cancel_outbox(
        &mut self,
        owner: &str,
        id: &str,
        now: &str,
    ) -> Result<bool, AuthStoreError> {
        Ok(self.connection.execute(
            "UPDATE outbox_items SET status='cancelled',updated_at=?1,lease_until=NULL
             WHERE user_id=?2 AND id=?3 AND status IN ('scheduled','failed')",
            params![now, owner, id],
        )? == 1)
    }

    pub fn retry_outbox(
        &mut self,
        owner: &str,
        id: &str,
        now: &str,
    ) -> Result<bool, AuthStoreError> {
        Ok(self.connection.execute(
            "UPDATE outbox_items SET status='scheduled',scheduled_at=?1,updated_at=?1,last_error_code=NULL,last_error_message=NULL,lease_until=NULL
             WHERE user_id=?2 AND id=?3 AND status='failed'",
            params![now,owner,id],
        )? == 1)
    }

    pub fn claim_due_outbox(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<Option<OutboxWorkItem>, AuthStoreError> {
        let now_text = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let lease =
            (now + Duration::minutes(LEASE_MINUTES)).to_rfc3339_opts(SecondsFormat::Millis, true);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE outbox_items SET status='needsReview',last_error_code='OUTBOX_RESULT_UNCERTAIN',
             last_error_message='服务在发送过程中中断，请核对已发送邮件后人工处理。',lease_until=NULL,updated_at=?1
             WHERE status='sending' AND lease_until IS NOT NULL AND lease_until<=?1",
            [&now_text],
        )?;
        let raw = tx.query_row(
            "SELECT id,user_id,message_json,draft_id FROM outbox_items
             WHERE status='scheduled' AND scheduled_at<=?1 ORDER BY scheduled_at,created_at,id LIMIT 1",
            [&now_text],
            |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,Option<String>>(3)?)),
        ).optional()?;
        let Some((id, owner, message_json, draft_id)) = raw else {
            tx.commit()?;
            return Ok(None);
        };
        if tx.execute(
            "UPDATE outbox_items SET status='sending',attempts=attempts+1,lease_until=?1,updated_at=?2
             WHERE id=?3 AND status='scheduled'",
            params![lease,now_text,id],
        )? != 1 { tx.commit()?; return Ok(None); }
        let message = serde_json::from_str(&message_json)?;
        tx.commit()?;
        Ok(Some(OutboxWorkItem {
            id,
            owner_id: owner,
            message,
            draft_id,
        }))
    }

    pub fn complete_outbox(
        &mut self,
        id: &str,
        message_id: &str,
        now: &str,
    ) -> Result<bool, AuthStoreError> {
        Ok(self.connection.execute(
            "UPDATE outbox_items SET status='sent',sent_message_id=?1,sent_at=?2,updated_at=?2,lease_until=NULL,last_error_code=NULL,last_error_message=NULL
             WHERE id=?3 AND status='sending'",
            params![message_id,now,id],
        )? == 1)
    }

    pub fn fail_outbox(
        &mut self,
        id: &str,
        code: &str,
        message: &str,
        uncertain: bool,
        now: &str,
    ) -> Result<bool, AuthStoreError> {
        let status = if uncertain { "needsReview" } else { "failed" };
        Ok(self.connection.execute(
            "UPDATE outbox_items SET status=?1,last_error_code=?2,last_error_message=?3,updated_at=?4,lease_until=NULL
             WHERE id=?5 AND status='sending'",
            params![status,code,message,now,id],
        )? == 1)
    }
}

type RawOutboxRow = (
    String,
    String,
    String,
    String,
    String,
    u32,
    Option<String>,
    String,
    String,
    Option<String>,
);

fn outbox_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawOutboxRow> {
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
        row.get(9)?,
    ))
}

fn decode_outbox_row(row: RawOutboxRow) -> Result<OutboxItemReadModel, AuthStoreError> {
    let message: SendMessageInput = serde_json::from_str(&row.2)?;
    let status = match row.4.as_str() {
        "scheduled" => OutboxStatus::Scheduled,
        "sending" => OutboxStatus::Sending,
        "sent" => OutboxStatus::Sent,
        "failed" => OutboxStatus::Failed,
        "needsReview" => OutboxStatus::NeedsReview,
        "cancelled" => OutboxStatus::Cancelled,
        _ => return Err(AuthStoreError::InvalidContentData),
    };
    Ok(OutboxItemReadModel {
        id: row.0,
        account_id: row.1,
        to: message.to,
        cc: message.cc.unwrap_or_default(),
        bcc: message.envelope.bcc,
        subject: message.subject,
        scheduled_at: row.3,
        status,
        attempts: row.5,
        last_error: row.6,
        created_at: row.7,
        updated_at: row.8,
        sent_at: row.9,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::{Duration, TimeZone, Utc};
    use imail_protocol::{ComposeEnvelope, OutboxStatus, SendMessageInput};
    use rusqlite::Connection;
    use uuid::Uuid;

    use super::*;

    fn database() -> (std::path::PathBuf, SqliteAuthStore) {
        let root = std::env::temp_dir().join(format!("imail-outbox-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("imail.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(&format!(
                "CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL) STRICT;
             INSERT INTO metadata VALUES('schema_version','{}');
             CREATE TABLE accounts(id TEXT PRIMARY KEY,user_id TEXT NOT NULL) STRICT;
             INSERT INTO accounts VALUES('account-1','owner-1');",
                imail_protocol::CURRENT_SCHEMA_VERSION
            ))
            .unwrap();
        connection
            .execute_batch(include_str!("../sql/migration-v15-outbox.sql"))
            .unwrap();
        drop(connection);
        let store = SqliteAuthStore::open_database(&database).unwrap();
        (root, store)
    }

    fn message() -> SendMessageInput {
        SendMessageInput {
            envelope: ComposeEnvelope::default(),
            account_id: "account-1".into(),
            to: vec!["recipient@example.com".into()],
            cc: None,
            subject: "Scheduled".into(),
            text: "Body".into(),
            html: None,
            attachments: None,
        }
    }

    #[test]
    fn schedules_claims_and_completes_an_immutable_snapshot() {
        let (root, mut store) = database();
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 8, 0, 0).unwrap();
        let now_text = now.to_rfc3339();
        let item = store
            .schedule_outbox(
                "owner-1",
                "item-1",
                &message(),
                Some("draft-1"),
                &now_text,
                &now_text,
            )
            .unwrap();
        assert_eq!(item.status, OutboxStatus::Scheduled);
        let work = store.claim_due_outbox(now).unwrap().unwrap();
        assert_eq!(work.message.subject, "Scheduled");
        assert!(!store.cancel_outbox("owner-1", "item-1", &now_text).unwrap());
        assert!(store
            .complete_outbox("item-1", "smtp-message-1", &now_text)
            .unwrap());
        assert_eq!(
            store.list_outbox("owner-1").unwrap()[0].status,
            OutboxStatus::Sent
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn expired_sending_lease_requires_review_instead_of_resending() {
        let (root, mut store) = database();
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 8, 0, 0).unwrap();
        store
            .schedule_outbox(
                "owner-1",
                "item-2",
                &message(),
                None,
                &now.to_rfc3339(),
                &now.to_rfc3339(),
            )
            .unwrap();
        assert!(store.claim_due_outbox(now).unwrap().is_some());
        assert!(store
            .claim_due_outbox(now + Duration::minutes(11))
            .unwrap()
            .is_none());
        let item = store.outbox_item("owner-1", "item-2").unwrap().unwrap();
        assert_eq!(item.status, OutboxStatus::NeedsReview);
        assert!(!store
            .retry_outbox("owner-1", "item-2", &now.to_rfc3339())
            .unwrap());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_from_v14_adds_the_outbox_schema() {
        let root = std::env::temp_dir().join(format!("imail-outbox-migration-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("imail.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL) STRICT; INSERT INTO metadata VALUES('schema_version','14');").unwrap();
        drop(connection);
        let report = crate::migrate_database(&database).unwrap();
        assert_eq!(report.applied_versions, [15]);
        let connection = Connection::open(&database).unwrap();
        let table: String = connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='outbox_items'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table, "outbox_items");
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }
}
