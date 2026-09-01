use super::*;
use imail_core::search::{SearchFilters, SmartFolderInput};

#[test]
fn search_dates_flags_labels_gateway_scope_and_account_deletion_are_consistent() {
    let fixture = Fixture::new(17);
    let mut store = SqliteAuthStore::open_database(fixture.root.join("imail.sqlite")).unwrap();
    let mut message = store.message("user-1", "m1").unwrap().unwrap();
    message.date = "2026-08-31T00:30:00Z".into();
    message.text = "searchable body".into();
    message.subject = "100%_budget".into();
    message.unread = false;
    message.flagged = true;
    message.has_attachments = false;
    message.labels = json!(["one", "two"]);
    store.upsert_message("user-1", &message).unwrap();
    let base = SearchFilters {
        since: Some("2026-08-31T08:00:00+08:00".into()),
        before: Some("2026-08-31T01:00:00Z".into()),
        body: Some("searchable".into()),
        subject: Some("%_".into()),
        labels: vec!["one".into(), "two".into()],
        unread: Some(false),
        flagged: Some(true),
        has_attachments: Some(false),
        ..Default::default()
    };
    let query = |filters| MessageQuery {
        filters: Some(filters),
        limit: 10,
        ..Default::default()
    };
    assert_eq!(
        store
            .query_messages("user-1", &query(base.clone()), "2026-08-31T02:00:00Z")
            .unwrap()
            .total,
        1
    );
    for filters in [
        SearchFilters {
            before: Some(message.date.clone()),
            ..base.clone()
        },
        SearchFilters {
            unread: Some(true),
            ..base.clone()
        },
        SearchFilters {
            flagged: Some(false),
            ..base.clone()
        },
        SearchFilters {
            has_attachments: Some(true),
            ..base.clone()
        },
        SearchFilters {
            labels: vec!["missing".into()],
            ..base.clone()
        },
        SearchFilters {
            account_ids: vec!["not-owned".into()],
            ..base.clone()
        },
    ] {
        assert_eq!(
            store
                .query_messages("user-1", &query(filters), "2026-08-31T02:00:00Z")
                .unwrap()
                .total,
            0
        );
    }
    let gateway = imail_core::messages::GatewayMessageQuery {
        account_ids: vec![message.account_id.clone()],
        filters: Some(base.clone()),
        limit: 10,
        ..Default::default()
    };
    assert_eq!(
        store
            .query_gateway_messages("user-1", &gateway)
            .unwrap()
            .messages
            .len(),
        1
    );
    assert!(store
        .query_gateway_messages("other-user", &gateway)
        .unwrap()
        .messages
        .is_empty());
    assert!(store
        .query_gateway_messages(
            "user-1",
            &imail_core::messages::GatewayMessageQuery {
                account_ids: vec!["not-permitted".into()],
                ..gateway
            }
        )
        .unwrap()
        .messages
        .is_empty());
    assert!(store.delete_account("user-1", &message.account_id).unwrap());
    assert_eq!(
        store
            .query_messages("user-1", &query(base), "2026-08-31T02:00:00Z")
            .unwrap()
            .total,
        0
    );
    let hits: i64 = store
        .connection
        .query_row(
            "SELECT count(*) FROM message_body_fts WHERE message_body_fts MATCH 'searchable'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hits, 0);
}

#[test]
fn body_index_records_large_cache_search_size_and_rebuild_cost() {
    let fixture = Fixture::new(17);
    let mut store = SqliteAuthStore::open_database(fixture.root.join("imail.sqlite")).unwrap();
    let started = std::time::Instant::now();
    let body = "常规内容 monthly archive. ".repeat(160);
    let transaction = store.connection.transaction().unwrap();
    {
        let mut insert = transaction.prepare("INSERT INTO messages(id,account_id,mailbox,mailbox_role,uid,from_json,to_json,subject,preview,text_body,received_at,unread,flagged,has_attachments,attachments_json,labels_json) VALUES(?1,'a-first','Archive','archive',?2,'{}','[]','Scale','Preview',?3,'2026-08-31T00:00:00Z',0,0,0,'[]','[]')").unwrap();
        for i in 0..10_000 {
            insert
                .execute(rusqlite::params![
                    format!("search-scale-{i}"),
                    i + 100,
                    format!("{body}独特预算 approval-{:05}", i)
                ])
                .unwrap();
        }
    }
    transaction.commit().unwrap();
    let insert_ms = started.elapsed().as_millis();
    let mut timings = Vec::new();
    for text in ["独特预算", "approval-09999", "预算 approval-09999", "预算"] {
        let started = std::time::Instant::now();
        let page = store
            .query_messages(
                "user-1",
                &MessageQuery {
                    filters: Some(SearchFilters {
                        body: Some(text.into()),
                        ..Default::default()
                    }),
                    limit: 60,
                    ..Default::default()
                },
                "2026-08-31T02:00:00Z",
            )
            .unwrap();
        assert_eq!(page.total, if text.contains("09999") { 1 } else { 10_000 });
        timings.push((text, started.elapsed().as_millis()));
        assert!(started.elapsed() < Duration::from_secs(10));
    }
    let index_bytes: i64 = store
        .connection
        .query_row(
            "SELECT coalesce(sum(length(block)),0) FROM message_body_fts_data",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let started = std::time::Instant::now();
    store.rebuild_body_search_index().unwrap();
    let rebuild_ms = started.elapsed().as_millis();
    let integrity = store.connection.execute(
        "INSERT INTO message_body_fts(message_body_fts,rank) VALUES('integrity-check',1)",
        [],
    );
    assert!(integrity.is_ok());
    println!("SEARCH_BENCH rows=10000 body_utf8_bytes={} insert_ms={insert_ms} index_payload_bytes={index_bytes} rebuild_ms={rebuild_ms} queries={timings:?}",body.len());
}

#[test]
fn advanced_search_is_literal_combined_paginated_and_owner_scoped() {
    let fixture = Fixture::new(17);
    let mut store = SqliteAuthStore::open_database(fixture.root.join("imail.sqlite")).unwrap();
    let mut message = store.message("user-1", "m1").unwrap().unwrap();
    message.text = "季度预算 ABC %_ \"OR\" 混合内容".into();
    message.unread = false;
    message.flagged = false;
    message.labels = json!(["finance", "review"]);
    message.headers.cc = vec![imail_protocol::MailAddressView {
        name: "Copy".into(),
        address: "copy@example.test".into(),
    }];
    store.upsert_message("user-1", &message).unwrap();
    let query = MessageQuery {
        filters: Some(SearchFilters {
            body: Some("预算".into()),
            unread: Some(false),
            flagged: Some(false),
            has_attachments: Some(message.has_attachments),
            labels: vec!["finance".into(), "review".into()],
            recipient: Some("COPY@example.test".into()),
            account_ids: vec![message.account_id.clone()],
            ..Default::default()
        }),
        limit: 1,
        ..Default::default()
    };
    assert_eq!(
        store
            .query_messages("user-1", &query, "2026-08-31T00:00:00Z")
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        store
            .query_messages("other-user", &query, "2026-08-31T00:00:00Z")
            .unwrap()
            .total,
        0
    );
    for term in ["季度预算", "abc", "%_", "\"OR\"", "混合内容"] {
        let query = MessageQuery {
            filters: Some(SearchFilters {
                body: Some(term.into()),
                ..Default::default()
            }),
            limit: 60,
            ..Default::default()
        };
        assert_eq!(
            store
                .query_messages("user-1", &query, "2026-08-31T00:00:00Z")
                .unwrap()
                .total,
            1,
            "{term}"
        );
    }
    let mut second = message.clone();
    second.id = "search-second".into();
    second.uid += 100;
    store.upsert_message("user-1", &second).unwrap();
    let page = store
        .query_messages("user-1", &query, "2026-08-31T00:00:00Z")
        .unwrap();
    assert_eq!(page.total, 2);
    assert!(page.has_more);
    let tail = store
        .query_messages(
            "user-1",
            &MessageQuery {
                cursor: Some(GatewayMessageCursor {
                    date: page.messages[0].date.clone(),
                    id: page.messages[0].id.clone(),
                }),
                ..query.clone()
            },
            "2026-08-31T00:00:00Z",
        )
        .unwrap();
    assert_eq!(tail.total, 2);
    assert_eq!(tail.messages.len(), 1);
    assert!(!tail.has_more);
    message.text = "changed".into();
    store.upsert_message("user-1", &message).unwrap();
    store.delete_message("user-1", &second.id).unwrap();
    assert_eq!(
        store
            .query_messages("user-1", &query, "2026-08-31T00:00:00Z")
            .unwrap()
            .total,
        0
    );
    store.rebuild_body_search_index().unwrap();
    assert_eq!(
        store
            .query_messages("user-1", &query, "2026-08-31T00:00:00Z")
            .unwrap()
            .total,
        0
    );
}

#[test]
fn migration_backfills_cached_bodies_and_is_idempotent() {
    let fixture = Fixture::new(12);
    let path = fixture.root.join("imail.sqlite");
    let report = migrate_database(&path).unwrap();
    assert_eq!(report.applied_versions, [13, 14, 15, 16, 17]);
    assert!(migrate_database(&path).unwrap().applied_versions.is_empty());
    let store = SqliteAuthStore::open_database(path).unwrap();
    let message = store.message("user-1", "m1").unwrap().unwrap();
    let query = MessageQuery {
        filters: Some(SearchFilters {
            body: Some(message.text),
            ..Default::default()
        }),
        limit: 60,
        ..Default::default()
    };
    assert_eq!(
        store
            .query_messages("user-1", &query, "2026-08-31T00:00:00Z")
            .unwrap()
            .total,
        1
    );
}

#[test]
fn smart_folders_persist_conditions_without_copying_mail_and_reject_foreign_accounts() {
    let fixture = Fixture::new(17);
    let path = fixture.root.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&path).unwrap();
    let user = store
        .create_user("search-owner", "Owner", "test-password-123")
        .unwrap();
    let input = SmartFolderInput {
        name: "未读待办".into(),
        filters: SearchFilters {
            unread: Some(true),
            ..Default::default()
        },
    };
    let created = store
        .save_smart_folder(&user.id, None, &input)
        .unwrap()
        .unwrap();
    assert!(store.list_smart_folders("other-user").unwrap().is_empty());
    assert!(store
        .save_smart_folder("user-1", Some(&created.id), &input)
        .is_err());
    assert!(!store
        .delete_smart_folder("other-user", &created.id)
        .unwrap());
    assert!(store
        .save_smart_folder(
            &user.id,
            None,
            &SmartFolderInput {
                filters: SearchFilters {
                    account_ids: vec!["a-first".into()],
                    ..Default::default()
                },
                ..input.clone()
            }
        )
        .is_err());
    drop(store);
    let mut store = SqliteAuthStore::open_database(&path).unwrap();
    assert_eq!(
        store.list_smart_folders(&user.id).unwrap(),
        vec![created.clone()]
    );
    let renamed = store
        .save_smart_folder(
            &user.id,
            Some(&created.id),
            &SmartFolderInput {
                name: "待处理".into(),
                ..input
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(renamed.name, "待处理");
    assert_eq!(renamed.created_at, created.created_at);
    assert!(store.delete_smart_folder(&user.id, &created.id).unwrap());
    assert!(store.list_smart_folders(&user.id).unwrap().is_empty());
    assert_eq!(store.list_messages("user-1").unwrap().len(), 1);
}
