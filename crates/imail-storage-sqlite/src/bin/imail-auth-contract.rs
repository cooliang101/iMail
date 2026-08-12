use std::{collections::BTreeMap, env, path::Path, process};

use imail_core::{
    AccountRecord, AccountRepository, AuthRepository, ContentRepository, DeveloperTokenRepository,
    LogoFetchAttemptRecord,
};
use imail_protocol::{ContactReadModel, DraftReadModel, MessageReadModel};
use imail_security::MasterKey;
use imail_storage_sqlite::SqliteAuthStore;
use serde_json::json;

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 6 {
        eprintln!(
            "用法：imail-auth-contract <database> <node-login> <node-password> <node-session> <node-token> <existing-account-id-or-empty>"
        );
        process::exit(2);
    }
    match exercise(&arguments) {
        Ok(output) => println!(
            "{}",
            serde_json::to_string(&output).expect("serialize contract")
        ),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}

fn exercise(arguments: &[String]) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut store = SqliteAuthStore::open_database(&arguments[0])?;
    let master_key = MasterKey::from_file(
        Path::new(&arguments[0])
            .parent()
            .ok_or("数据库路径缺少父目录")?
            .join("master.key"),
    )?;
    let existing_credential_reencrypted = if arguments[5].is_empty() {
        false
    } else {
        store.reencrypt_account_credential(&arguments[5], &master_key)?
    };
    let password_user = store.authenticate(&arguments[1], &arguments[2])?;
    let session_user = store.user_for_session(&arguments[3])?;
    let node_token = store.authenticate_developer_token(&arguments[4], "messages:read")?;
    let node_account = if let Some(user) = &password_user {
        store.account(&user.id, "11111111-1111-4111-8111-111111111111")?
    } else {
        None
    };
    let node_account_secret_accepted = node_account.as_ref().is_some_and(|account| {
        master_key
            .decrypt_json::<serde_json::Value>(&account.encrypted_secret)
            .ok()
            .and_then(|secret| secret.get("password").cloned())
            .is_some_and(|password| password == "Node contract mailbox password")
    });
    let node_metadata_accepted = if let Some(user) = &password_user {
        store
            .user_metadata(&user.id, "app_preferences_v1")?
            .is_some_and(|value| value.contains("rendered"))
    } else {
        false
    };
    let node_content_accepted = if let Some(user) = &password_user {
        store
            .list_messages(&user.id)?
            .iter()
            .any(|item| item.id == "node-contract-message")
            && store
                .list_drafts(&user.id)?
                .iter()
                .any(|item| item.id == "node-contract-draft")
            && store
                .list_contacts(&user.id)?
                .iter()
                .any(|item| item.address.eq_ignore_ascii_case("sender@example.test"))
            && store
                .list_logo_fetch_attempts(&user.id)?
                .iter()
                .any(|item| item.target == "sender@example.test")
    } else {
        false
    };
    let node_rate_limit = store.consume_attempt("node-contract-rate-limit", 3, 60_000)?;
    let rust_password = "Rust contract password 456!";
    let rust_user = store.create_user(
        "rust.contract@example.test",
        "Rust Contract User",
        rust_password,
    )?;
    let rust_session = store.create_session(&rust_user.id)?;
    let rust_account = AccountRecord {
        id: "22222222-2222-4222-8222-222222222222".into(),
        owner_id: rust_user.id.clone(),
        provider: "custom".into(),
        email: "rust.contract.mailbox@example.test".into(),
        display_name: "Rust Contract Mailbox".into(),
        group: "Contract".into(),
        group_icon: "code".into(),
        color: "#168f78".into(),
        settings: json!({
            "imapHost": "imap.example.test", "imapPort": 993, "imapSecure": true,
            "smtpHost": "smtp.example.test", "smtpPort": 465, "smtpSecure": true
        }),
        proxy: None,
        encrypted_secret: master_key.encrypt_json(&json!({
            "authType": "oauth2", "accessToken": "Rust contract access token"
        }))?,
        auth_method: Some("oauth2".into()),
        created_at: "2026-08-10T00:00:00.000Z".into(),
        last_sync_at: None,
        status: "connected".into(),
        last_error: None,
        mailboxes: json!([]),
    };
    store.upsert_account(&rust_account)?;
    store.set_user_metadata(
        &rust_user.id,
        "app_preferences_v1",
        r#"{"defaultMessageView":"source"}"#,
    )?;
    let rust_message = MessageReadModel {
        id: "rust-contract-message".into(),
        account_id: rust_account.id.clone(),
        mailbox: "INBOX".into(),
        mailbox_role: "inbox".into(),
        uid: 42,
        message_id: Some("<rust-contract@example.test>".into()),
        from: json!({ "name": "Rust Sender", "address": "rust.sender@example.test" }),
        to: json!([{ "name": "Rust User", "address": rust_account.email }]),
        subject: "Rust contract message".into(),
        preview: "Rust preview".into(),
        text: "Rust body".into(),
        html: Some("<p>Rust body</p>".into()),
        date: "2026-08-10T04:00:00.000Z".into(),
        unread: true,
        flagged: false,
        has_attachments: false,
        attachments: json!([]),
        labels: json!(["contract"]),
        snoozed_until: None,
    };
    store.upsert_message(&rust_user.id, &rust_message)?;
    let rust_draft = DraftReadModel {
        id: "rust-contract-draft".into(),
        account_id: rust_account.id.clone(),
        to: json!(["recipient@example.test"]),
        cc: json!([]),
        subject: "Rust contract draft".into(),
        text: "Rust draft body".into(),
        html: "<p>Rust draft body</p>".into(),
        attachments: json!([]),
        created_at: "2026-08-10T05:00:00.000Z".into(),
        updated_at: "2026-08-10T05:00:00.000Z".into(),
    };
    store.upsert_draft(&rust_user.id, &rust_draft)?;
    store.upsert_contact(&ContactReadModel {
        owner_id: rust_user.id.clone(),
        address: "rust.person@example.test".into(),
        name: "Rust Person".into(),
        message_count: 1,
        last_contact_at: "2026-08-10T04:00:00.000Z".into(),
        logo_key: None,
        logo_content_type: None,
        logo_source_url: None,
        logo_fetched_at: None,
    })?;
    store.upsert_logo_fetch_attempt(&LogoFetchAttemptRecord {
        owner_id: rust_user.id.clone(),
        target: "rust.person@example.test".into(),
        domain_key: "example.test".into(),
        status: "failed".into(),
        detail: "contract".into(),
        attempted_at: "2026-08-10T04:00:00.000Z".into(),
    })?;
    let rust_token = store.issue_developer_token(
        &rust_user.id,
        "Rust MCP Contract Token",
        &["mcp:full".into()],
        &[],
        3_600,
    )?;
    let cross_user_revoke_rejected = if let Some(user) = &password_user {
        !store.revoke_developer_token(&user.id, &rust_token.token.id)?
    } else {
        false
    };
    let mut detail = BTreeMap::new();
    detail.insert("source".into(), "rust-contract".into());
    store.record_security_event(
        "rust.contract.created",
        "192.0.2.99",
        Some(&rust_user.id),
        &detail,
    )?;
    let rust_rate_limit_first = store.consume_attempt("rust-contract-rate-limit", 2, 60_000)?;
    let rust_rate_limit_second = store.consume_attempt("rust-contract-rate-limit", 2, 60_000)?;
    Ok(json!({
        "nodePasswordAccepted": password_user.is_some(),
        "existingCredentialReencrypted": existing_credential_reencrypted,
        "nodeSessionAccepted": session_user.is_some(),
        "nodeTokenAccepted": node_token.is_some(),
        "nodeAccountAccepted": node_account_secret_accepted,
        "nodeMetadataAccepted": node_metadata_accepted,
        "nodeContentAccepted": node_content_accepted,
        "nodeRateLimitAccepted": node_rate_limit.allowed,
        "rustRateLimitPrimed": rust_rate_limit_first.allowed && rust_rate_limit_second.allowed,
        "rustUserId": rust_user.id,
        "rustLogin": rust_user.login,
        "rustPassword": rust_password,
        "rustSession": rust_session,
        "rustTokenId": rust_token.token.id,
        "rustToken": rust_token.raw,
        "rustAccountId": rust_account.id,
        "rustMessageId": rust_message.id,
        "rustDraftId": rust_draft.id,
        "crossUserTokenRevokeRejected": cross_user_revoke_rejected,
    }))
}
