use super::*;

#[tokio::test]
async fn discovers_persists_and_reuses_a_contact_logo_without_network_in_tests() {
    let directory = authentication_directory("logo-discovery");
    let database = directory.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let owner = store
        .create_user("logo-owner", "Logo Owner", "owner-password-123")
        .unwrap();
    let session = store.create_session(&owner.id).unwrap();
    store
        .upsert_contact(&imail_protocol::ContactReadModel {
            owner_id: owner.id.clone(),
            address: "sender@example.org".into(),
            name: "Sender".into(),
            message_count: 1,
            last_contact_at: "2026-08-10T08:00:00.000Z".into(),
            logo_key: None,
            logo_content_type: None,
            logo_source_url: None,
            logo_fetched_at: None,
        })
        .unwrap();
    drop(store);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let router = build_router(
        HttpAdapterConfig::new(&directory).with_logo_discovery(Arc::new(TestLogoDiscovery {
            calls: Arc::clone(&calls),
        })),
    )
    .unwrap();
    let request = || {
        Request::builder()
            .uri("/api/contacts/logo?address=sender%40example.org")
            .header(HOST, "127.0.0.1:8787")
            .header("cookie", format!("imail_session={session}"))
            .body(Body::empty())
            .unwrap()
    };
    for _ in 0..2 {
        let response = router.clone().oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "image/png");
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let store = SqliteAuthStore::open_database(&database).unwrap();
    let contact = store.list_contacts(&owner.id).unwrap().remove(0);
    assert_eq!(contact.logo_key.as_deref(), Some("domain:example.org"));
    assert_eq!(store.list_logo_fetch_attempts(&owner.id).unwrap().len(), 1);
    drop(store);
    fs::remove_dir_all(directory).unwrap();
}
