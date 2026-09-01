use chrono::{DateTime, Duration, SecondsFormat, Utc};
use imail_protocol::{OutboxItemReadModel, OutboxStatus, SendMessageInput};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::{AuthStoreError, SqliteAuthStore};

const LEASE_MINUTES: i64 = 10;
const TERMINAL_HISTORY_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxWorkItem {
    pub id: String,
    pub owner_id: String,
    pub message: SendMessageInput,
}

pub struct OutboxSchedule<'a> {
    pub id: &'a str,
    pub message: &'a SendMessageInput,
    pub draft_id: Option<&'a str>,
    pub idempotency_key: &'a str,
    pub scheduled_at: &'a str,
    pub now: &'a str,
}

impl SqliteAuthStore {
    pub fn schedule_outbox(
        &mut self,
        owner: &str,
        input: OutboxSchedule<'_>,
    ) -> Result<OutboxItemReadModel, AuthStoreError> {
        let OutboxSchedule {
            id,
            message,
            draft_id,
            idempotency_key,
            scheduled_at,
            now,
        } = input;
        let message_json = serde_json::to_string(message)?;
        let request_fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(
                &message_json,
                draft_id.unwrap_or_default(),
                scheduled_at,
            ))?)
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owned = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND user_id=?2)",
            params![message.account_id, owner],
            |row| row.get::<_, bool>(0),
        )?;
        if !owned {
            return Err(AuthStoreError::AccountNotOwned);
        }
        let existing = tx
            .query_row(
                "SELECT id,request_fingerprint FROM outbox_items
                 WHERE user_id=?1 AND idempotency_key=?2",
                params![owner, idempotency_key],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()?;
        let item_id = if let Some((existing_id, existing_fingerprint)) = existing {
            if existing_fingerprint.as_deref() != Some(&request_fingerprint) {
                return Err(AuthStoreError::IdempotencyConflict);
            }
            existing_id
        } else {
            tx.execute(
                "INSERT INTO outbox_items(id,user_id,account_id,message_json,draft_id,idempotency_key,request_fingerprint,scheduled_at,status,created_at,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'scheduled',?9,?9)",
                params![id, owner, message.account_id, message_json, draft_id, idempotency_key, request_fingerprint, scheduled_at, now],
            )?;
            id.to_string()
        };
        tx.commit()?;
        self.outbox_item(owner, &item_id)?
            .ok_or(AuthStoreError::InvalidContentData)
    }

    pub fn list_outbox(&self, owner: &str) -> Result<Vec<OutboxItemReadModel>, AuthStoreError> {
        let mut statement = self.connection.prepare(
            "SELECT id,account_id,message_json,scheduled_at,status,attempts,last_error_message,created_at,updated_at,sent_at
             FROM outbox_items WHERE user_id=?1 AND (
               status IN ('scheduled','sending','failed','needsReview') OR id IN (
                 SELECT id FROM outbox_items WHERE user_id=?1
                 AND status IN ('sent','cancelled')
                 ORDER BY updated_at DESC,id DESC LIMIT ?2
               )
             )
             ORDER BY CASE status WHEN 'sending' THEN 0 WHEN 'scheduled' THEN 1 WHEN 'needsReview' THEN 2 WHEN 'failed' THEN 3 ELSE 4 END,
                      scheduled_at DESC,created_at DESC",
        )?;
        let rows = statement
            .query_map(params![owner, TERMINAL_HISTORY_LIMIT], outbox_row)?
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
            "UPDATE outbox_items SET status='cancelled',updated_at=?1,lease_until=NULL,
             message_json=json_set(message_json,'$.text','','$.html',NULL,'$.attachments',NULL)
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

    pub fn resolve_outbox(
        &mut self,
        owner: &str,
        id: &str,
        was_sent: bool,
        now: &str,
    ) -> Result<bool, AuthStoreError> {
        let status = if was_sent { "sent" } else { "cancelled" };
        let sent_at = was_sent.then_some(now);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = tx.execute(
            "UPDATE outbox_items SET status=?1,sent_at=?2,updated_at=?3,lease_until=NULL,
             last_error_code=NULL,last_error_message=NULL,
             message_json=json_set(message_json,'$.text','','$.html',NULL,'$.attachments',NULL)
             WHERE user_id=?4 AND id=?5 AND status='needsReview'",
            params![status, sent_at, now, owner, id],
        )? == 1;
        if updated && was_sent {
            tx.execute(
                "DELETE FROM drafts WHERE id=(SELECT draft_id FROM outbox_items WHERE user_id=?1 AND id=?2)",
                params![owner, id],
            )?;
        }
        tx.commit()?;
        Ok(updated)
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
            "SELECT id,user_id,message_json FROM outbox_items
             WHERE status='scheduled' AND scheduled_at<=?1 ORDER BY scheduled_at,created_at,id LIMIT 1",
            [&now_text],
            |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?)),
        ).optional()?;
        let Some((id, owner, message_json)) = raw else {
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
        }))
    }

    pub fn complete_outbox(
        &mut self,
        id: &str,
        message_id: &str,
        now: &str,
    ) -> Result<bool, AuthStoreError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = tx.execute(
            "UPDATE outbox_items SET status='sent',sent_message_id=?1,sent_at=?2,updated_at=?2,
             lease_until=NULL,last_error_code=NULL,last_error_message=NULL,
             message_json=json_set(message_json,'$.text','','$.html',NULL,'$.attachments',NULL)
             WHERE id=?3 AND status='sending'",
            params![message_id, now, id],
        )? == 1;
        if updated {
            tx.execute(
                "DELETE FROM drafts WHERE id=(SELECT draft_id FROM outbox_items WHERE id=?1)",
                [id],
            )?;
        }
        tx.commit()?;
        Ok(updated)
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

    pub fn prune_outbox(&mut self, before: &str) -> Result<usize, AuthStoreError> {
        Ok(self.connection.execute(
            "DELETE FROM outbox_items WHERE status IN ('sent','cancelled') AND updated_at<?1",
            [before],
        )?)
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
             INSERT INTO accounts VALUES('account-1','owner-1');
             CREATE TABLE drafts(id TEXT PRIMARY KEY) STRICT;",
                imail_protocol::CURRENT_SCHEMA_VERSION
            ))
            .unwrap();
        connection
            .execute_batch(include_str!("../sql/migration-v15-outbox.sql"))
            .unwrap();
        connection
            .execute_batch(include_str!("../sql/migration-v16-outbox-idempotency.sql"))
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
        store
            .connection
            .execute("INSERT INTO drafts(id) VALUES('draft-1')", [])
            .unwrap();
        let item = store
            .schedule_outbox(
                "owner-1",
                OutboxSchedule {
                    id: "item-1",
                    message: &message(),
                    draft_id: Some("draft-1"),
                    idempotency_key: "request-1",
                    scheduled_at: &now_text,
                    now: &now_text,
                },
            )
            .unwrap();
        assert_eq!(item.status, OutboxStatus::Scheduled);
        let work = store.claim_due_outbox(now).unwrap().unwrap();
        assert_eq!(work.message.subject, "Scheduled");
        assert!(!store.cancel_outbox("owner-1", "item-1", &now_text).unwrap());
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_draft_delete BEFORE DELETE ON drafts
                 BEGIN SELECT RAISE(ABORT,'simulated cleanup failure'); END;",
            )
            .unwrap();
        assert!(store
            .complete_outbox("item-1", "smtp-message-1", &now_text)
            .is_err());
        assert_eq!(
            store
                .outbox_item("owner-1", "item-1")
                .unwrap()
                .unwrap()
                .status,
            OutboxStatus::Sending
        );
        store
            .connection
            .execute_batch("DROP TRIGGER reject_draft_delete")
            .unwrap();
        assert!(store
            .complete_outbox("item-1", "smtp-message-1", &now_text)
            .unwrap());
        assert_eq!(
            store.list_outbox("owner-1").unwrap()[0].status,
            OutboxStatus::Sent
        );
        let draft_count: u32 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM drafts WHERE id='draft-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(draft_count, 0);
        let repeated = store
            .schedule_outbox(
                "owner-1",
                OutboxSchedule {
                    id: "item-duplicate-after-send",
                    message: &message(),
                    draft_id: Some("draft-1"),
                    idempotency_key: "request-1",
                    scheduled_at: &now_text,
                    now: &now_text,
                },
            )
            .unwrap();
        assert_eq!(repeated.id, "item-1");
        assert_eq!(repeated.status, OutboxStatus::Sent);
        let snapshot: String = store
            .connection
            .query_row(
                "SELECT message_json FROM outbox_items WHERE id='item-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!snapshot.contains("Body"));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scheduling_is_idempotent_and_rejects_key_reuse_with_different_content() {
        let (root, mut store) = database();
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 8, 0, 0).unwrap();
        let send_at = (now + Duration::hours(1)).to_rfc3339();
        let first = store
            .schedule_outbox(
                "owner-1",
                OutboxSchedule {
                    id: "item-idempotent-1",
                    message: &message(),
                    draft_id: None,
                    idempotency_key: "request-idempotent",
                    scheduled_at: &send_at,
                    now: &now.to_rfc3339(),
                },
            )
            .unwrap();
        let repeated = store
            .schedule_outbox(
                "owner-1",
                OutboxSchedule {
                    id: "item-idempotent-2",
                    message: &message(),
                    draft_id: None,
                    idempotency_key: "request-idempotent",
                    scheduled_at: &send_at,
                    now: &now.to_rfc3339(),
                },
            )
            .unwrap();
        assert_eq!(repeated.id, first.id);
        let mut changed = message();
        changed.subject = "Different".into();
        assert!(matches!(
            store.schedule_outbox(
                "owner-1",
                OutboxSchedule {
                    id: "item-idempotent-3",
                    message: &changed,
                    draft_id: None,
                    idempotency_key: "request-idempotent",
                    scheduled_at: &send_at,
                    now: &now.to_rfc3339(),
                },
            ),
            Err(AuthStoreError::IdempotencyConflict)
        ));
        assert!(store
            .cancel_outbox("owner-1", &first.id, &now.to_rfc3339())
            .unwrap());
        store
            .connection
            .execute(
                "UPDATE outbox_items SET updated_at='2026-01-01T00:00:00Z' WHERE id=?1",
                [&first.id],
            )
            .unwrap();
        assert_eq!(store.prune_outbox("2026-06-01T00:00:00Z").unwrap(), 1);
        assert!(store.outbox_item("owner-1", &first.id).unwrap().is_none());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn expired_sending_lease_requires_review_instead_of_resending() {
        let (root, mut store) = database();
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 8, 0, 0).unwrap();
        store
            .connection
            .execute("INSERT INTO drafts(id) VALUES('draft-return')", [])
            .unwrap();
        store
            .schedule_outbox(
                "owner-1",
                OutboxSchedule {
                    id: "item-2",
                    message: &message(),
                    draft_id: Some("draft-return"),
                    idempotency_key: "request-2",
                    scheduled_at: &now.to_rfc3339(),
                    now: &now.to_rfc3339(),
                },
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
        assert!(store
            .resolve_outbox("owner-1", "item-2", false, &now.to_rfc3339())
            .unwrap());
        assert_eq!(
            store
                .outbox_item("owner-1", "item-2")
                .unwrap()
                .unwrap()
                .status,
            OutboxStatus::Cancelled
        );
        let draft_count: u32 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM drafts WHERE id='draft-return'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(draft_count, 1);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uncertain_delivery_can_be_marked_as_manually_verified_sent() {
        let (root, mut store) = database();
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 8, 0, 0).unwrap();
        store
            .connection
            .execute("INSERT INTO drafts(id) VALUES('draft-sent')", [])
            .unwrap();
        store
            .schedule_outbox(
                "owner-1",
                OutboxSchedule {
                    id: "item-verified",
                    message: &message(),
                    draft_id: Some("draft-sent"),
                    idempotency_key: "request-verified",
                    scheduled_at: &now.to_rfc3339(),
                    now: &now.to_rfc3339(),
                },
            )
            .unwrap();
        assert!(store.claim_due_outbox(now).unwrap().is_some());
        assert!(store
            .fail_outbox(
                "item-verified",
                "MAIL_PROTOCOL_ERROR",
                "uncertain",
                true,
                &now.to_rfc3339(),
            )
            .unwrap());
        assert!(store
            .resolve_outbox("owner-1", "item-verified", true, &now.to_rfc3339(),)
            .unwrap());
        let item = store
            .outbox_item("owner-1", "item-verified")
            .unwrap()
            .unwrap();
        assert_eq!(item.status, OutboxStatus::Sent);
        assert_eq!(item.sent_at.as_deref(), Some(now.to_rfc3339().as_str()));
        let draft_count: u32 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM drafts WHERE id='draft-sent'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(draft_count, 0);
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
        assert_eq!(report.applied_versions, [15, 16]);
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

    #[test]
    fn migration_from_v15_adds_idempotency_and_compacts_terminal_snapshots() {
        let root = std::env::temp_dir().join(format!("imail-outbox-v16-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("imail.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL) STRICT;
                 INSERT INTO metadata VALUES('schema_version','15');
                 CREATE TABLE accounts(id TEXT PRIMARY KEY,user_id TEXT NOT NULL) STRICT;
                 INSERT INTO accounts VALUES('account-1','owner-1');",
            )
            .unwrap();
        connection
            .execute_batch(include_str!("../sql/migration-v15-outbox.sql"))
            .unwrap();
        connection
            .execute(
                "INSERT INTO outbox_items(id,user_id,account_id,message_json,scheduled_at,status,created_at,updated_at)
                 VALUES('terminal','owner-1','account-1',?1,'2026-01-01T00:00:00Z','sent','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
                [serde_json::to_string(&message()).unwrap()],
            )
            .unwrap();
        drop(connection);
        let report = crate::migrate_database(&database).unwrap();
        assert_eq!(report.applied_versions, [16]);
        let connection = Connection::open(&database).unwrap();
        let snapshot: String = connection
            .query_row(
                "SELECT message_json FROM outbox_items WHERE id='terminal'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!snapshot.contains("Body"));
        let columns = connection
            .prepare("PRAGMA table_info(outbox_items)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.contains(&"idempotency_key".to_string()));
        assert!(columns.contains(&"request_fingerprint".to_string()));
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }
}
