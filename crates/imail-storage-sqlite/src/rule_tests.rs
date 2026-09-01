use super::*;
use imail_core::rules::{matching_rules, MailRuleInput, RuleAction, RuleCondition, RuleMatchMode};

#[test]
fn sync_rules_skip_initial_history_then_apply_once_to_new_inbox_mail_before_notifications() {
    let (fixture, mut store) = setup();
    let database = fixture.root.join("imail.sqlite");
    migrate_database(&database).unwrap();
    store.save_mail_rule("user-1", None, &input()).unwrap();
    let mut message = store.message("user-1", "m1").unwrap().unwrap();
    message.id = "initial-import".into();
    message.uid = 8;
    message.message_id = Some("<initial@example.com>".into());
    let mut sync = crate::SyncRuntimeStore::open_database(&database).unwrap();
    let mut plan = imail_mail::MailboxSyncPlan {
        account_id: "a-first".into(),
        mailbox: "INBOX".into(),
        mailbox_role: "inbox".into(),
        uid_validity: Some("1".into()),
        highest_modseq: None,
        last_seen_uid: 8,
        incoming: vec![message.clone()],
        raw_sources: Default::default(),
        removed_uids: vec![],
        uid_validity_changed: false,
        flag_updates: vec![],
        folders: vec![],
        synced: 1,
        updated: 0,
        deleted: 0,
    };
    sync.commit_mailbox_sync(&plan, chrono::Utc::now()).unwrap();
    assert!(store.mail_rule_runs("user-1").unwrap().is_empty());
    store.connection.execute("UPDATE mailbox_sync_states SET last_success_at='2026-08-30T00:00:00Z' WHERE account_id='a-first'",[]).unwrap();
    message.id = "new-arrival".into();
    message.uid = 9;
    message.message_id = Some("<new@example.com>".into());
    plan.incoming = vec![message.clone()];
    plan.last_seen_uid = 9;
    let committed = sync.commit_mailbox_sync(&plan, chrono::Utc::now()).unwrap();
    assert_eq!(store.mail_rule_runs("user-1").unwrap().len(), 1);
    assert_eq!(
        committed.created_messages[0].labels,
        json!(["important", "开发通知"])
    );
    sync.record_message_created(
        "a-first",
        "first@example.com",
        &committed.created_messages,
        chrono::Utc::now(),
    )
    .unwrap();
    assert_eq!(
        sync.events(0, 100)
            .unwrap()
            .iter()
            .filter(|e| e.event_type == "message.created")
            .count(),
        0
    );
    sync.commit_mailbox_sync(&plan, chrono::Utc::now()).unwrap();
    assert_eq!(store.mail_rule_runs("user-1").unwrap().len(), 1);
    store.delete_message("user-1", "new-arrival").unwrap();
    message.id = "new-after-uid-reset".into();
    message.uid = 100;
    plan.incoming = vec![message];
    plan.uid_validity_changed = true;
    sync.commit_mailbox_sync(&plan, chrono::Utc::now()).unwrap();
    assert_eq!(store.mail_rule_runs("user-1").unwrap().len(), 1);
}

fn input() -> MailRuleInput {
    MailRuleInput {
        name: "开发通知".into(),
        enabled: true,
        priority: 100,
        account_ids: vec![],
        match_mode: RuleMatchMode::All,
        conditions: vec![RuleCondition::SenderDomain("example.com".into())],
        actions: vec![
            RuleAction::AddLabel("开发通知".into()),
            RuleAction::Mute(true),
        ],
        stop_processing: false,
    }
}

#[test]
fn editing_or_deleting_running_rules_cancels_remaining_actions_even_after_restart() {
    for delete in [false, true] {
        for interrupted in [false, true] {
            let (_fixture, mut store) = setup();
            let mut rule = input();
            rule.actions = vec![RuleAction::MarkRead(true), RuleAction::Archive];
            let saved = store
                .save_mail_rule("user-1", None, &rule)
                .unwrap()
                .unwrap();
            let preview = store
                .preview_mail_rule("user-1", &rule, Some(&saved.id))
                .unwrap();
            store
                .apply_mail_rule_preview("user-1", preview.token.as_deref().unwrap())
                .unwrap();
            let work = store
                .claim_rule_action("user-1", "a-first")
                .unwrap()
                .unwrap();
            if delete {
                store.delete_mail_rule("user-1", &saved.id).unwrap();
            } else {
                rule.enabled = false;
                store
                    .save_mail_rule("user-1", Some(&saved.id), &rule)
                    .unwrap();
            }
            if interrupted {
                store
                    .connection
                    .execute(
                        "UPDATE mail_rule_runs SET lease_until='2000-01-01T00:00:00Z' WHERE id=?1",
                        [&work.id],
                    )
                    .unwrap();
            } else {
                store.finish_rule_action("user-1", &work, Ok(None)).unwrap();
            }
            assert!(store
                .claim_rule_action("user-1", "a-first")
                .unwrap()
                .is_none());
            assert_eq!(
                store.mail_rule_runs("user-1").unwrap()[0].status,
                "cancelled"
            );
            assert_eq!(
                store.message("user-1", "m1").unwrap().unwrap().mailbox,
                "INBOX"
            );
        }
    }
}
fn setup() -> (Fixture, SqliteAuthStore) {
    let fixture = Fixture::new(15);
    let store = SqliteAuthStore::open_database(fixture.root.join("imail.sqlite")).unwrap();
    (fixture, store)
}

#[test]
fn rule_validation_matching_and_priority_are_deterministic() {
    let (_fixture, mut store) = setup();
    let mut message = store.message("user-1", "m1").unwrap().unwrap();
    message.subject = "开发 INVOICE %_".into();
    message.headers.cc = vec![imail_protocol::MailAddressView {
        name: "财务".into(),
        address: "finance@example.com".into(),
    }];
    let mut rule = input();
    rule.conditions
        .push(RuleCondition::SubjectContains("invoice %_".into()));
    rule.conditions
        .push(RuleCondition::Recipient("FINANCE@example.com".into()));
    assert!(rule.matches(&message));
    rule.conditions
        .push(RuleCondition::SenderDomain("ample.com".into()));
    assert!(!rule.matches(&message));
    rule.match_mode = RuleMatchMode::Any;
    assert!(rule.matches(&message));
    rule.account_ids = vec!["a-later".into()];
    assert!(!rule.matches(&message));
    rule.account_ids.clear();
    rule.conditions.clear();
    assert!(rule.validate().is_err());
    assert!(!rule.matches(&message));
    rule = input();
    rule.actions = vec![RuleAction::Archive, RuleAction::Mute(true)];
    assert!(rule.validate().is_err());
    assert!(serde_json::from_value::<RuleAction>(
        json!({"type":"forward","value":"attacker@test.com"})
    )
    .is_err());
    let later = store
        .save_mail_rule("user-1", None, &input())
        .unwrap()
        .unwrap();
    let mut stop = input();
    stop.priority = 1;
    stop.stop_processing = true;
    let first = store
        .save_mail_rule("user-1", None, &stop)
        .unwrap()
        .unwrap();
    let rules = vec![later, first.clone()];
    assert_eq!(
        matching_rules(&rules, &message)
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        [first.id.as_str()]
    );
}

#[test]
fn rule_preview_is_read_only_confirmation_is_exact_and_owner_scoped() {
    let (_fixture, mut store) = setup();
    let rule = store
        .save_mail_rule("user-1", None, &input())
        .unwrap()
        .unwrap();
    assert!(store.list_mail_rules("other").unwrap().is_empty());
    assert!(store
        .save_mail_rule("other", Some(&rule.id), &input())
        .unwrap()
        .is_none());
    assert!(!store.delete_mail_rule("other", &rule.id).unwrap());
    let before = store.message("user-1", "m1").unwrap().unwrap();
    let preview = store
        .preview_mail_rule("user-1", &rule.input, Some(&rule.id))
        .unwrap();
    assert_eq!(preview.total, 1);
    assert_eq!(preview.eligible, 1);
    assert!(store.mail_rule_runs("user-1").unwrap().is_empty());
    assert_eq!(store.message("user-1", "m1").unwrap().unwrap(), before);
    assert!(store
        .apply_mail_rule_preview("other", preview.token.as_deref().unwrap())
        .is_err());
    assert_eq!(
        store
            .apply_mail_rule_preview("user-1", preview.token.as_deref().unwrap())
            .unwrap(),
        1
    );
    assert!(store
        .apply_mail_rule_preview("user-1", preview.token.as_deref().unwrap())
        .is_err());
    let after = store.message("user-1", "m1").unwrap().unwrap();
    assert_eq!(after.labels, json!(["important", "开发通知"]));
    assert!(store.rule_message_muted("user-1", &after).unwrap());
    assert!(store.notification_messages("user-1").unwrap().is_empty());
    let run = store.mail_rule_runs("user-1").unwrap().remove(0);
    assert_eq!(run.status, "succeeded");
    assert_eq!(run.completed_actions, 2);
    let repeat = store
        .preview_mail_rule("user-1", &rule.input, Some(&rule.id))
        .unwrap();
    assert_eq!(repeat.total, 1);
    assert_eq!(repeat.eligible, 0);
}

#[test]
fn stale_preview_rejects_message_or_rule_edits_without_partial_actions() {
    let (_fixture, mut store) = setup();
    let rule = store
        .save_mail_rule("user-1", None, &input())
        .unwrap()
        .unwrap();
    let preview = store
        .preview_mail_rule("user-1", &rule.input, Some(&rule.id))
        .unwrap();
    let mut message = store.message("user-1", "m1").unwrap().unwrap();
    message.unread = false;
    store.upsert_message("user-1", &message).unwrap();
    assert!(store
        .apply_mail_rule_preview("user-1", preview.token.as_deref().unwrap())
        .is_err());
    assert!(store.mail_rule_runs("user-1").unwrap().is_empty());
    let preview = store
        .preview_mail_rule("user-1", &rule.input, Some(&rule.id))
        .unwrap();
    store
        .save_mail_rule("user-1", Some(&rule.id), &rule.input)
        .unwrap();
    assert!(store
        .apply_mail_rule_preview("user-1", preview.token.as_deref().unwrap())
        .is_err());
    let mut foreign = input();
    foreign.account_ids = vec!["foreign-account".into()];
    assert!(store.save_mail_rule("user-1", None, &foreign).is_err());
    assert!(store.preview_mail_rule("user-1", &foreign, None).is_err());
}

#[test]
fn remote_actions_checkpoint_restart_and_never_blindly_replay_archive() {
    let (fixture, mut store) = setup();
    let mut rule = input();
    rule.actions = vec![
        RuleAction::MarkRead(true),
        RuleAction::Flag(true),
        RuleAction::Archive,
    ];
    let saved = store
        .save_mail_rule("user-1", None, &rule)
        .unwrap()
        .unwrap();
    let preview = store
        .preview_mail_rule("user-1", &rule, Some(&saved.id))
        .unwrap();
    store
        .apply_mail_rule_preview("user-1", preview.token.as_deref().unwrap())
        .unwrap();
    let work = store
        .claim_rule_action("user-1", "a-first")
        .unwrap()
        .unwrap();
    assert_eq!(work.index, 0);
    assert!(store.message("user-1", "m1").unwrap().unwrap().unread);
    assert!(store
        .claim_rule_action("user-1", "a-first")
        .unwrap()
        .is_none());
    store.finish_rule_action("user-1", &work, Ok(None)).unwrap();
    drop(store);
    let mut store = SqliteAuthStore::open_database(fixture.root.join("imail.sqlite")).unwrap();
    let work = store
        .claim_rule_action("user-1", "a-first")
        .unwrap()
        .unwrap();
    assert_eq!(work.index, 1);
    assert!(!store.message("user-1", "m1").unwrap().unwrap().unread);
    store.finish_rule_action("user-1", &work, Ok(None)).unwrap();
    let work = store
        .claim_rule_action("user-1", "a-first")
        .unwrap()
        .unwrap();
    assert_eq!(work.action, RuleAction::Archive);
    store
        .connection
        .execute(
            "UPDATE mail_rule_runs SET lease_until='2000-01-01T00:00:00Z' WHERE id=?1",
            [&work.id],
        )
        .unwrap();
    assert!(store
        .claim_rule_action("user-1", "a-first")
        .unwrap()
        .is_none());
    let run = store.mail_rule_runs("user-1").unwrap().remove(0);
    assert_eq!(run.status, "needsReview");
    assert_eq!(run.completed_actions, 2);
    assert!(!store.retry_mail_rule_run("user-1", &run.id).unwrap());
    assert_eq!(
        store.message("user-1", "m1").unwrap().unwrap().mailbox,
        "INBOX"
    );
}

#[test]
fn retries_are_bounded_errors_sanitized_and_delete_cancels_pending_work() {
    let (_fixture, mut store) = setup();
    let mut rule = input();
    rule.actions = vec![RuleAction::Flag(true)];
    let saved = store
        .save_mail_rule("user-1", None, &rule)
        .unwrap()
        .unwrap();
    let preview = store
        .preview_mail_rule("user-1", &rule, Some(&saved.id))
        .unwrap();
    store
        .apply_mail_rule_preview("user-1", preview.token.as_deref().unwrap())
        .unwrap();
    for _ in 0..3 {
        let work = store
            .claim_rule_action("user-1", "a-first")
            .unwrap()
            .unwrap();
        store
            .finish_rule_action("user-1", &work, Err("password=secret token=private"))
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE mail_rule_runs SET lease_until=NULL WHERE id=?1",
                [&work.id],
            )
            .unwrap();
    }
    let run = store.mail_rule_runs("user-1").unwrap().remove(0);
    assert_eq!(run.status, "failed");
    assert_eq!(run.error_code.as_deref(), Some("REMOTE_ACTION_FAILED"));
    assert!(!store.retry_mail_rule_run("other", &run.id).unwrap());
    assert!(store.retry_mail_rule_run("user-1", &run.id).unwrap());
    store.delete_mail_rule("user-1", &saved.id).unwrap();
    assert!(store
        .claim_rule_action("user-1", "a-first")
        .unwrap()
        .is_none());
    assert_eq!(
        store.mail_rule_runs("user-1").unwrap()[0].status,
        "cancelled"
    );
}

#[test]
fn original_snapshot_matching_local_conflicts_and_dedup_are_stable() {
    let (_fixture, mut store) = setup();
    let first = store
        .save_mail_rule("user-1", None, &input())
        .unwrap()
        .unwrap();
    let mut second = input();
    second.priority = 200;
    second.actions = vec![
        RuleAction::RemoveLabel("开发通知".into()),
        RuleAction::Mute(false),
    ];
    let second = store
        .save_mail_rule("user-1", None, &second)
        .unwrap()
        .unwrap();
    let mut third = input();
    third.priority = 300;
    third.conditions = vec![RuleCondition::Label("开发通知".into())];
    let third = store
        .save_mail_rule("user-1", None, &third)
        .unwrap()
        .unwrap();
    let mut message = store.message("user-1", "m1").unwrap().unwrap();
    let rules = vec![third, second, first];
    let tx = store.connection.transaction().unwrap();
    assert_eq!(
        crate::rules::enqueue_matches(
            &tx,
            "user-1",
            &rules,
            &mut message,
            "automatic",
            "2026-08-31T00:00:00Z"
        )
        .unwrap(),
        2
    );
    assert_eq!(message.labels, json!(["important"]));
    assert!(!crate::rules::is_muted(&tx, &message).unwrap());
    assert_eq!(
        crate::rules::enqueue_matches(
            &tx,
            "user-1",
            &rules,
            &mut message,
            "automatic",
            "2026-08-31T00:00:00Z"
        )
        .unwrap(),
        0
    );
    tx.commit().unwrap();
    assert_eq!(store.mail_rule_runs("user-1").unwrap().len(), 2);
}
