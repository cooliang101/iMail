use super::*;

#[tokio::test]
async fn isolates_sync_policies_status_and_jobs_by_session_owner() {
    let directory = authentication_directory("sync-control");
    let database = directory.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let owner = store
        .create_user("sync-owner", "Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("sync-other", "Other", "other-password-123")
        .unwrap();
    let owner_session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    let owner_account_id = Uuid::new_v4().to_string();
    let other_account_id = Uuid::new_v4().to_string();
    let account = |id: String, owner_id: String, email: &str, mailboxes: Value| AccountRecord {
        id,
        owner_id,
        provider: "custom".into(),
        email: email.into(),
        display_name: email.into(),
        group: "个人".into(),
        group_icon: "folder".into(),
        color: "#168f78".into(),
        settings: json!({}),
        proxy: None,
        encrypted_secret: "encrypted".into(),
        auth_method: Some("app-password".into()),
        created_at: "2026-08-10T00:00:00.000Z".into(),
        last_sync_at: None,
        status: "connected".into(),
        last_error: None,
        mailboxes,
    };
    store
        .upsert_account(&account(
            owner_account_id.clone(),
            owner.id.clone(),
            "owner@example.com",
            json!([
                {"path":"INBOX","specialUse":"\\Inbox","selectable":true},
                {"path":"Archive","specialUse":"\\Archive","selectable":true},
                {"path":"Hidden","selectable":false}
            ]),
        ))
        .unwrap();
    store
        .upsert_account(&account(
            other_account_id.clone(),
            other.id.clone(),
            "other@example.com",
            json!([]),
        ))
        .unwrap();
    drop(store);
    let mut sync = SyncRuntimeStore::open_database(&database).unwrap();
    let owner_job = sync
        .enqueue(
            &SyncEnqueue {
                account_id: owner_account_id.clone(),
                mailbox: None,
                mailbox_role: "inbox".into(),
                reason: "manual".into(),
                priority: 100,
                not_before: None,
            },
            chrono::Utc::now(),
        )
        .unwrap();
    let other_job = sync
        .enqueue(
            &SyncEnqueue {
                account_id: other_account_id.clone(),
                mailbox: None,
                mailbox_role: "inbox".into(),
                reason: "manual".into(),
                priority: 100,
                not_before: None,
            },
            chrono::Utc::now(),
        )
        .unwrap();
    let owner_event_job = sync
        .enqueue(
            &SyncEnqueue {
                account_id: owner_account_id.clone(),
                mailbox: None,
                mailbox_role: "sent".into(),
                reason: "manual".into(),
                priority: 200,
                not_before: None,
            },
            chrono::Utc::now(),
        )
        .unwrap();
    let other_event_job = sync
        .enqueue(
            &SyncEnqueue {
                account_id: other_account_id.clone(),
                mailbox: None,
                mailbox_role: "sent".into(),
                reason: "manual".into(),
                priority: 199,
                not_before: None,
            },
            chrono::Utc::now(),
        )
        .unwrap();
    let claimed_owner = sync
        .claim_next(
            "sse-worker",
            std::time::Duration::from_secs(30),
            chrono::Utc::now(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(claimed_owner.id, owner_event_job.id);
    sync.mark_started(&claimed_owner, "Sent", chrono::Utc::now())
        .unwrap();
    let claimed_other = sync
        .claim_next(
            "sse-worker",
            std::time::Duration::from_secs(30),
            chrono::Utc::now(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(claimed_other.id, other_event_job.id);
    sync.mark_started(&claimed_other, "Sent", chrono::Utc::now())
        .unwrap();
    drop(sync);
    let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();

    let defaults = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/sync-policy")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let defaults = json(defaults).await;
    assert_eq!(defaults["policy"]["folderMode"], "inbox");
    assert_eq!(defaults["policy"]["notifyOnError"], true);

    let updated_default = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri("/api/sync-policy")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(
                    r#"{"folderMode":"standard","notifyOnError":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let updated_default = json(updated_default).await;
    assert_eq!(updated_default["policy"]["folderMode"], "standard");
    assert_eq!(updated_default["policy"]["notifyOnError"], false);

    let isolated_default = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/sync-policy")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        json(isolated_default).await["policy"]["folderMode"],
        "inbox"
    );

    let forbidden_policy = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/accounts/{owner_account_id}/sync-policy"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden_policy.status(), StatusCode::NOT_FOUND);

    let account_policy = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("/api/accounts/{owner_account_id}/sync-policy"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(
                    r#"{"folderMode":"selected","selectedMailboxes":[" Archive "]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(account_policy.status(), StatusCode::OK);
    let account_policy = json(account_policy).await;
    assert_eq!(account_policy["policy"]["folderMode"], "selected");
    assert_eq!(
        account_policy["policy"]["selectedMailboxes"],
        json!(["Archive"])
    );

    let invalid_selection = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("/api/accounts/{owner_account_id}/sync-policy"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(r#"{"selectedMailboxes":["Hidden"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_selection.status(), StatusCode::BAD_REQUEST);

    let status = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/sync-status")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = json(status).await;
    assert_eq!(status["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(status["accounts"][0]["accountId"], owner_account_id);
    assert!(status["accounts"][0]["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|job| job["id"] == owner_job.id));
    assert!(status.to_string().find(&other_account_id).is_none());
    assert!(status.to_string().find(&other_job.id).is_none());

    let event_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/events?after=0")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        event_response.headers()[CONTENT_TYPE],
        "text/event-stream; charset=utf-8"
    );
    assert_eq!(
        event_response.headers()["cache-control"],
        "no-cache, no-transform"
    );
    let mut event_stream = event_response.into_body().into_data_stream();
    let initial = tokio::time::timeout(std::time::Duration::from_secs(2), event_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let initial = String::from_utf8(initial.to_vec()).unwrap();
    assert!(initial.contains("event: connected"));
    assert!(initial.contains("event: sync.started"));
    assert!(initial.contains("event: sync.status"));
    assert!(initial.contains(&owner_event_job.id));
    assert!(!initial.contains(&other_event_job.id));
    assert!(!initial.contains(&other_account_id));
    let mut live_sync = SyncRuntimeStore::open_database(&database).unwrap();
    let live_job = live_sync
        .enqueue(
            &SyncEnqueue {
                account_id: owner_account_id.clone(),
                mailbox: None,
                mailbox_role: "archive".into(),
                reason: "manual".into(),
                priority: 300,
                not_before: None,
            },
            chrono::Utc::now(),
        )
        .unwrap();
    let claimed_live = live_sync
        .claim_next(
            "sse-worker",
            std::time::Duration::from_secs(30),
            chrono::Utc::now(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(claimed_live.id, live_job.id);
    live_sync
        .mark_started(&claimed_live, "Archive", chrono::Utc::now())
        .unwrap();
    drop(live_sync);
    let live = tokio::time::timeout(std::time::Duration::from_secs(3), event_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let live = String::from_utf8(live.to_vec()).unwrap();
    assert!(live.contains("event: sync.started"));
    assert!(live.contains(&live_job.id));
    assert!(live.contains("event: sync.status"));
    drop(event_stream);

    let resumed = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/events?after=0")
                .header(HOST, "127.0.0.1:8787")
                .header("last-event-id", "9223372036854775807")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut resumed_stream = resumed.into_body().into_data_stream();
    let resumed = resumed_stream.next().await.unwrap().unwrap();
    let resumed = String::from_utf8(resumed.to_vec()).unwrap();
    assert!(resumed.contains("event: connected"));
    assert!(!resumed.contains("event: sync.started"));
    drop(resumed_stream);

    let owned_job = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/sync-jobs/{}", owner_job.id))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(owned_job.status(), StatusCode::OK);

    let foreign_job = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/sync-jobs/{}", other_job.id))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(foreign_job.status(), StatusCode::NOT_FOUND);

    let disabled = router
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("/api/accounts/{owner_account_id}/sync-policy"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(r#"{"enabled":false}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json(disabled).await["policy"]["enabled"], false);
    assert_eq!(
        SyncRuntimeStore::open_database(&database)
            .unwrap()
            .job(&owner_job.id)
            .unwrap()
            .unwrap()
            .status,
        "cancelled"
    );
    fs::remove_dir_all(directory).unwrap();
}
