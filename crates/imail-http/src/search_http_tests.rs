use super::*;

#[tokio::test]
async fn smart_folder_routes_enforce_sessions_ownership_and_strict_conditions() {
    let directory = authentication_directory("smart-folders");
    let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
    let owner = store
        .create_user("search-owner", "Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("search-other", "Other", "other-password-123")
        .unwrap();
    let session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    drop(store);
    let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();
    let request = |method: Method, uri: &str, session: &str, body: Value| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "127.0.0.1:8787")
            .header(CONTENT_TYPE, "application/json")
            .header("cookie", format!("imail_session={session}"))
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let response = router
        .clone()
        .oneshot(request(Method::GET, "/api/smart-folders", "", Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let input = serde_json::json!({"name":"待处理","filters":{"body":"项目预算","unread":false}});
    let response = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/smart-folders",
            &session,
            input.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let saved = json(response).await;
    let id = saved["folder"]["id"].as_str().unwrap();
    let url = format!("/api/smart-folders/{id}");
    let foreign = router
        .clone()
        .oneshot(request(Method::PUT, &url, &other_session, input.clone()))
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    let foreign = router
        .clone()
        .oneshot(request(Method::DELETE, &url, &other_session, Value::Null))
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    let list = json(
        router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/smart-folders",
                &other_session,
                Value::Null,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(list["folders"], serde_json::json!([]));
    for filters in [
        serde_json::json!({"unexpected":true}),
        serde_json::json!({"since":"not-date"}),
        serde_json::json!({"accountIds":["foreign"]}),
    ] {
        let response = router
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/smart-folders",
                &session,
                serde_json::json!({"name":"无效","filters":filters}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let renamed = json(
        router
            .clone()
            .oneshot(request(
                Method::PUT,
                &url,
                &session,
                serde_json::json!({"name":"项目","filters":saved["folder"]["filters"]}),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(renamed["folder"]["name"], "项目");
    assert_eq!(renamed["folder"]["filters"]["unread"], false);
    let deleted = router
        .oneshot(request(Method::DELETE, &url, &session, Value::Null))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::OK);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn embedded_and_http_search_queries_use_the_same_strict_filter_parser() {
    let fields = std::collections::BTreeMap::from([(
        "filters".into(),
        r#"{"body":"项目","unread":false}"#.into(),
    )]);
    let query = crate::messages::embedded_message_query(&fields).unwrap();
    assert_eq!(query.mailbox_role, None);
    assert_eq!(query.filters.unwrap().unread, Some(false));
    for raw in [
        r#"{"ignoredTypo":1}"#,
        r#"{"before":"bad"}"#,
        r#"{"sender":"wrong"}"#,
    ] {
        assert!(
            crate::messages::embedded_message_query(&std::collections::BTreeMap::from([(
                "filters".into(),
                raw.into()
            )]))
            .is_err()
        );
    }
}
