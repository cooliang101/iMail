//! Executes the durable rule outbox through the same remote mail domain service.
use imail_core::rules::RuleAction;
use imail_mail::{
    ImapPort, MailConnectionConfig, OutgoingMessage, ProtocolFailure, ProtocolStage,
    RemoteMailService, RemoteMessageLocator, SmtpPort,
};
use imail_protocol::{MessageMoveDestination, RemoteMessageFlagPatch, SendMessageResult};
use imail_storage_sqlite::SqliteAuthStore;

/// Net metadata changes across both rule execution and sync. Sending the sync-only
/// snapshot would resurrect archived/unread rows in normal client folders.
pub(crate) fn message_changes(
    before: Vec<imail_protocol::MessageReadModel>,
    after: Vec<imail_protocol::MessageReadModel>,
) -> Vec<imail_mail::MailboxMessageChange> {
    let mut before = before
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut changes = Vec::new();
    for message in after {
        let previous = before.remove(&message.id);
        if previous.as_ref() != Some(&message) {
            changes.push(imail_mail::MailboxMessageChange {
                before: previous,
                after: Some(message),
            });
        }
    }
    changes.extend(
        before
            .into_values()
            .map(|m| imail_mail::MailboxMessageChange {
                before: Some(m),
                after: None,
            }),
    );
    changes
}

pub(crate) fn drain_rules(
    store: &mut SqliteAuthStore,
    owner: &str,
    account: &str,
    config: &MailConnectionConfig,
    imap: &mut dyn ImapPort,
    cancelled: impl Fn() -> bool,
) -> Result<(), String> {
    let mut unused = UnusedSmtp;
    // Cache is bounded to 5000 messages per account. Yield back to cancellation
    // between actions, and let persistent sync reconciliation resume pending work.
    for _ in 0..10000 {
        if cancelled() {
            break;
        }
        let Some(work) = store
            .claim_rule_action(owner, account)
            .map_err(|_| "规则任务读取失败")?
        else {
            break;
        };
        let Some(uid) = u32::try_from(work.message.uid).ok().filter(|uid| *uid > 0) else {
            store
                .finish_rule_action(owner, &work, Err("MESSAGE_NOT_FOUND"))
                .map_err(|_| "规则执行结果保存失败")?;
            continue;
        };
        let locator = RemoteMessageLocator {
            mailbox: work.message.mailbox.clone(),
            uid,
            message_id: work.message.message_id.clone(),
        };
        let mut service = RemoteMailService::new(&mut *imap, &mut unused);
        let result = match work.action {
            RuleAction::MarkRead(read) => service
                .update_flags(
                    config,
                    &locator,
                    &RemoteMessageFlagPatch {
                        unread: Some(!read),
                        flagged: None,
                    },
                )
                .map(|_| None),
            RuleAction::Flag(flag) => service
                .update_flags(
                    config,
                    &locator,
                    &RemoteMessageFlagPatch {
                        unread: None,
                        flagged: Some(flag),
                    },
                )
                .map(|_| None),
            RuleAction::Archive if work.message.mailbox_role == "archive" => {
                Ok(Some(imail_protocol::RemoteMessageMoveResult {
                    mailbox: work.message.mailbox.clone(),
                    uid: Some(locator.uid),
                }))
            }
            RuleAction::Archive => service
                .move_message(config, &locator, MessageMoveDestination::Archive)
                .map(Some),
            _ => unreachable!("only remote actions enter the outbox"),
        };
        let result = match result {
            Ok(Some(moved)) if moved.uid.is_none() => Err("MOVE_UNCONFIRMED"),
            Ok(value) => Ok(value),
            Err(_) => Err("REMOTE_ACTION_FAILED"),
        };
        store
            .finish_rule_action(owner, &work, result)
            .map_err(|_| "规则执行结果保存失败")?;
    }
    Ok(())
}

struct UnusedSmtp;
impl SmtpPort for UnusedSmtp {
    fn verify(&mut self, _: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
        Err(unused())
    }
    fn send(
        &mut self,
        _: &MailConnectionConfig,
        _: &OutgoingMessage,
    ) -> Result<SendMessageResult, ProtocolFailure> {
        Err(unused())
    }
}
fn unused() -> ProtocolFailure {
    ProtocolFailure::from_provider(
        ProtocolStage::Imap,
        Some("UNUSED_TRANSPORT"),
        "规则不允许发送邮件",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use imail_core::{
        rules::{MailRuleInput, RuleCondition, RuleMatchMode},
        ContentRepository, MessageRepository,
    };
    use imail_mail::{MailAuthentication, RemoteMailbox, RemoteMoveConfirmation};
    use imail_protocol::MessageReadModel;
    use serde_json::json;

    #[derive(Default)]
    struct MailFixture {
        calls: Vec<String>,
        confirm_move: bool,
    }
    impl ImapPort for MailFixture {
        fn verify(&mut self, _: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
            panic!("rules must not verify or send mail")
        }
        fn fetch_source(
            &mut self,
            _: &MailConnectionConfig,
            _: &RemoteMessageLocator,
        ) -> Result<Vec<u8>, ProtocolFailure> {
            panic!("rules use cached bodies")
        }
        fn update_flags(
            &mut self,
            _: &MailConnectionConfig,
            locator: &RemoteMessageLocator,
            patch: &RemoteMessageFlagPatch,
        ) -> Result<(), ProtocolFailure> {
            assert_eq!(locator.uid, 7);
            assert_eq!(locator.mailbox, "INBOX");
            self.calls.push(
                if patch.unread == Some(false) {
                    "read"
                } else {
                    "flag"
                }
                .into(),
            );
            Ok(())
        }
        fn list_mailboxes(
            &mut self,
            _: &MailConnectionConfig,
        ) -> Result<Vec<RemoteMailbox>, ProtocolFailure> {
            Ok(vec![RemoteMailbox {
                path: "Archive".into(),
                special_use: Some("\\Archive".into()),
            }])
        }
        fn move_message(
            &mut self,
            _: &MailConnectionConfig,
            locator: &RemoteMessageLocator,
            target: &str,
        ) -> Result<RemoteMoveConfirmation, ProtocolFailure> {
            assert_eq!(locator.uid, 7);
            assert_eq!(target, "Archive");
            self.calls.push("archive".into());
            Ok(RemoteMoveConfirmation {
                confirmed: self.confirm_move,
                uid: self.confirm_move.then_some(99),
            })
        }
    }

    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn fixture() -> (Fixture, SqliteAuthStore, MailConnectionConfig) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "imail-rule-runtime-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let db = rusqlite::Connection::open(&path).unwrap();
        drop(db);
        imail_storage_sqlite::migrate_database(&path).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute("INSERT INTO accounts(id,provider,email,display_name,group_name,color,settings_json,encrypted_secret,created_at,status,user_id) VALUES('account','custom','owner@example.test','Owner','personal','#000','{}','fixture-only','2026-08-31T00:00:00Z','connected','owner')",[]).unwrap();
        drop(db);
        let mut store = SqliteAuthStore::open_database(&path).unwrap();
        let message = MessageReadModel {
            headers: Default::default(),
            id: "message".into(),
            account_id: "account".into(),
            mailbox: "INBOX".into(),
            mailbox_role: "inbox".into(),
            uid: 7,
            message_id: Some("<test@example.test>".into()),
            from: json!({"address":"sender@example.test"}),
            to: json!([]),
            subject: "Invoice".into(),
            preview: "".into(),
            text: "body".into(),
            html: None,
            date: "2026-08-31T00:00:00Z".into(),
            unread: true,
            flagged: false,
            has_attachments: false,
            attachments: json!([]),
            labels: json!([]),
            snoozed_until: None,
        };
        store.upsert_message("owner", &message).unwrap();
        let rule = MailRuleInput {
            name: "Archive invoices".into(),
            enabled: true,
            priority: 1,
            account_ids: vec![],
            match_mode: RuleMatchMode::All,
            conditions: vec![RuleCondition::SubjectContains("invoice".into())],
            actions: vec![
                RuleAction::MarkRead(true),
                RuleAction::Flag(true),
                RuleAction::Archive,
            ],
            stop_processing: false,
        };
        let saved = store.save_mail_rule("owner", None, &rule).unwrap().unwrap();
        let preview = store
            .preview_mail_rule("owner", &rule, Some(&saved.id))
            .unwrap();
        store
            .apply_mail_rule_preview("owner", preview.token.as_deref().unwrap())
            .unwrap();
        let config = MailConnectionConfig {
            email: "owner@example.test".into(),
            display_name: "Owner".into(),
            imap_host: "fixture.invalid".into(),
            imap_port: 993,
            imap_secure: true,
            smtp_host: "fixture.invalid".into(),
            smtp_port: 465,
            smtp_secure: true,
            authentication: MailAuthentication::Password("synthetic".into()),
            proxy: None,
        };
        (Fixture(path), store, config)
    }

    #[test]
    fn rules_use_remote_domain_operations_and_persist_confirmed_results_once() {
        let (_fixture, mut store, config) = fixture();
        let mut mail = MailFixture {
            confirm_move: true,
            ..Default::default()
        };
        let before = store.conversation_candidates("owner").unwrap();
        drain_rules(&mut store, "owner", "account", &config, &mut mail, || false).unwrap();
        assert_eq!(mail.calls, ["read", "flag", "archive"]);
        let message = store.message("owner", "message").unwrap().unwrap();
        assert!(!message.unread);
        assert!(message.flagged);
        assert_eq!(message.mailbox, "Archive");
        assert_eq!(message.uid, 99);
        let changes = message_changes(before, store.conversation_candidates("owner").unwrap());
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].before.as_ref().unwrap().mailbox_role, "inbox");
        assert_eq!(changes[0].after.as_ref().unwrap().mailbox_role, "archive");
        assert!(!changes[0].after.as_ref().unwrap().unread);
        assert!(changes[0].after.as_ref().unwrap().text.is_empty());
        assert_eq!(
            store.mail_rule_runs("owner").unwrap()[0].status,
            "succeeded"
        );
        drain_rules(&mut store, "owner", "account", &config, &mut mail, || false).unwrap();
        assert_eq!(mail.calls.len(), 3);
    }

    #[test]
    fn unconfirmed_remote_archive_is_not_faked_locally_or_retried() {
        let (_fixture, mut store, config) = fixture();
        let mut mail = MailFixture::default();
        drain_rules(&mut store, "owner", "account", &config, &mut mail, || false).unwrap();
        assert_eq!(
            store.message("owner", "message").unwrap().unwrap().mailbox,
            "INBOX"
        );
        assert_eq!(
            store.mail_rule_runs("owner").unwrap()[0].status,
            "needsReview"
        );
        drain_rules(&mut store, "owner", "account", &config, &mut mail, || false).unwrap();
        assert_eq!(mail.calls, ["read", "flag", "archive"]);
    }
}
