use super::*;

#[tokio::test]
async fn rule_routes_validate_authority_preview_and_explicit_confirmation() {
    let directory = authentication_directory("mail-rules");
    let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
    let user = store
        .create_user("rules-owner", "Owner", "rules-password-123")
        .unwrap();
    let other = store
        .create_user("rules-other", "Other", "rules-password-456")
        .unwrap();
    let session = store.create_session(&user.id).unwrap();
    let foreign = store.create_session(&other.id).unwrap();
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
        .oneshot(request(Method::GET, "/api/mail-rules", "", Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let input = serde_json::json!({"name":"财务","enabled":false,"priority":10,"accountIds":[],"matchMode":"all","conditions":[{"field":"subjectContains","value":"发票"}],"actions":[{"type":"addLabel","value":"财务"}],"stopProcessing":true});
    let response = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/mail-rules",
            &session,
            input.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let saved = json(response).await;
    let id = saved["rule"]["id"].as_str().unwrap();
    let path = format!("/api/mail-rules/{id}");
    let response = router
        .clone()
        .oneshot(request(Method::DELETE, &path, &foreign, Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/mail-rules/preview",
            &session,
            serde_json::json!({"ruleId":id,"input":input}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let preview = json(response).await;
    let token = preview["token"].as_str().unwrap();
    for (session, confirmed, expected) in [
        (&session, false, StatusCode::BAD_REQUEST),
        (&foreign, true, StatusCode::BAD_REQUEST),
        (&session, true, StatusCode::OK),
        (&session, true, StatusCode::BAD_REQUEST),
    ] {
        let response = router
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/mail-rules/apply",
                session,
                serde_json::json!({"token":token,"confirmed":confirmed}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    for patch in [
        serde_json::json!({"conditions":[]}),
        serde_json::json!({"actions":[{"type":"deletePermanently"}]}),
        serde_json::json!({"accountIds":["other-account"]}),
        serde_json::json!({"unexpected":true}),
    ] {
        let mut bad = input.clone();
        bad.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        let response = router
            .clone()
            .oneshot(request(Method::PUT, &path, &session, bad))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
