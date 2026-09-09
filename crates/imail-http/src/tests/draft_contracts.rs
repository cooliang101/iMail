use super::*;

#[tokio::test]
async fn creates_updates_lists_and_deletes_only_owned_drafts() {
    let directory = authentication_directory("drafts");
    let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
    let owner = store
        .create_user("draft-http-owner", "Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("draft-http-other", "Other", "other-password-123")
        .unwrap();
    let owner_session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    let account_id = Uuid::new_v4().to_string();
    store
        .upsert_account(&AccountRecord {
            id: account_id.clone(),
            owner_id: owner.id.clone(),
            provider: "custom".into(),
            email: "draft-owner@example.com".into(),
            display_name: "Draft Owner".into(),
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
            mailboxes: json!([]),
        })
        .unwrap();
    drop(store);
    let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();
    let draft_id = Uuid::new_v4().to_string();

    let created = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/drafts")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("x-draft-id", &draft_id)
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(format!(
                    r#"{{"accountId":"{account_id}","to":[" recipient@example.com "],"subject":"First"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json(created).await;
    assert_eq!(created["draft"]["id"], draft_id);
    assert_eq!(created["draft"]["to"], json!(["recipient@example.com"]));
    assert_eq!(created["draft"]["cc"], json!([]));
    assert_eq!(created["draft"]["html"], "");
    let created_at = created["draft"]["createdAt"].clone();

    let replaced = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/drafts")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("x-draft-id", &draft_id)
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(format!(
                    r#"{{"accountId":"{account_id}","subject":"Replaced"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let replaced = json(replaced).await;
    assert_eq!(replaced["draft"]["createdAt"], created_at);
    assert_eq!(replaced["draft"]["subject"], "Replaced");

    let isolated = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/drafts")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json(isolated).await["drafts"], json!([]));

    let forbidden_update = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/drafts/{draft_id}"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::from(format!(
                    r#"{{"accountId":"{account_id}","subject":"Stolen"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden_update.status(), StatusCode::NOT_FOUND);

    let oversized = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/drafts")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(format!(
                    r#"{{"accountId":"{account_id}","attachments":[{{"id":"a","filename":"a.bin","contentType":"application/octet-stream","size":5242881,"data":""}}]}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(oversized.status(), StatusCode::BAD_REQUEST);

    let foreign_delete = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/drafts/{draft_id}"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(foreign_delete.status(), StatusCode::NO_CONTENT);

    let removed = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/drafts/{draft_id}"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);
    assert!(
        SqliteAuthStore::open_database(directory.join("imail.sqlite"))
            .unwrap()
            .list_drafts(&owner.id)
            .unwrap()
            .is_empty()
    );
    fs::remove_dir_all(directory).unwrap();
}
