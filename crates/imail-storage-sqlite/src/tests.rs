use std::{
    cell::Cell,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use imail_core::{
    accounts::{AccountService, CredentialValidationError},
    authorization_export::AuthorizationExportService,
    contacts::{
        contacts_need_logo_update, reconcile_contacts, ContactsService, RegistrableDomainResolver,
    },
    drafts::DraftService,
    maintenance::DataMaintenanceService,
    messages::{GatewayMessageCursor, MessageQuery},
    notifications::{build_notifications, list_labels, MailOverviewService},
    preferences::PreferencesService,
    privacy::PrivacyService,
    theme::CustomThemeService,
    translation_settings::{TranslationCredentialSecret, TranslationSettingsService},
    translations::TranslationService,
    AccountRecord, AccountRepository, AuthRepository, ContentRepository, DeveloperTokenRepository,
    LogoFetchAttemptRecord, MessageRepository, ReadOnlyRepository, TranslationCacheRepository,
    TranslationProviderRepository,
};
use imail_protocol::{
    AccountMetadataPatch, AccountProxyUpdate, AppPreferences, AppPreferencesPatch,
    ContactReadModel, CustomTheme, DraftInput, DraftReadModel, MailAuthorizationExportPayload,
    MessageReadModel, MessageView, NotificationKind, NotificationKindsPatch, ProxyProtocol,
    ShortcutBindingsPatch, ThemeId, TranslatedSegment, TranslationArtifact, TranslationCacheKey,
    TranslationCompletionRequest, TranslationCredentialKind, TranslationExecutionTarget,
    TranslationProviderConfiguration, TranslationProviderProfileInput, WorkspaceIconId,
    MAIL_AUTHORIZATION_EXPORT_FORMAT, MAIL_AUTHORIZATION_EXPORT_VERSION,
};
use imail_security::{decrypt_portable_export, sha256_hex, MasterKey, PortableEncryptedPayload};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::json;

use super::{migrate_database, MigrationError};
use super::{
    prepare_data_restore, AppleHmeAddressRecord, AuthStoreError, FilesystemDataMaintenance,
    MasterKeyCredentialCodec, PortableAuthorizationExportEncryptor, SqliteAuthStore,
    SqliteReadOnlyStore, StorageError,
};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDomains;

impl RegistrableDomainResolver for TestDomains {
    fn registrable_domain(&self, hostname: &str) -> Option<String> {
        let labels = hostname.split('.').collect::<Vec<_>>();
        (labels.len() >= 2).then(|| labels[labels.len() - 2..].join("."))
    }
}

#[test]
fn rust_matches_the_shared_r3_node_domain_fixture() {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureAccount {
        id: String,
        email: String,
        display_name: String,
        status: String,
        last_error: Option<String>,
        created_at: String,
        last_sync_at: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureLogo {
        key: String,
        content_type: String,
        source_url: String,
        fetched_at: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureContact {
        address: String,
        name: String,
        message_count: i64,
        last_contact_at: String,
        logo: FixtureLogo,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ExpectedContact {
        address: String,
        name: String,
        message_count: i64,
        last_contact_at: String,
        logo_key: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Expected {
        contact: ExpectedContact,
        labels: Vec<String>,
        notification_ids: Vec<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureData {
        now: String,
        accounts: Vec<FixtureAccount>,
        messages: Vec<MessageReadModel>,
        previous_contacts: Vec<FixtureContact>,
        expected: Expected,
    }

    let fixture: FixtureData =
        serde_json::from_str(include_str!("../../../fixtures/r3-domain-v1.json")).unwrap();
    let accounts = fixture
        .accounts
        .into_iter()
        .map(|account| AccountRecord {
            id: account.id,
            owner_id: "fixture-user".into(),
            provider: "custom".into(),
            email: account.email,
            display_name: account.display_name,
            group: "Fixture".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: json!({}),
            proxy: None,
            encrypted_secret: "not-observed".into(),
            auth_method: None,
            created_at: account.created_at,
            last_sync_at: account.last_sync_at,
            status: account.status,
            last_error: account.last_error,
            mailboxes: json!([]),
        })
        .collect::<Vec<_>>();
    let previous = fixture
        .previous_contacts
        .into_iter()
        .map(|contact| ContactReadModel {
            owner_id: "fixture-user".into(),
            address: contact.address,
            name: contact.name,
            message_count: contact.message_count,
            last_contact_at: contact.last_contact_at,
            logo_key: Some(contact.logo.key),
            logo_content_type: Some(contact.logo.content_type),
            logo_source_url: Some(contact.logo.source_url),
            logo_fetched_at: Some(contact.logo.fetched_at),
        })
        .collect::<Vec<_>>();
    let contacts = reconcile_contacts(
        "fixture-user",
        &accounts,
        &fixture.messages,
        &previous,
        &TestDomains,
    );
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].address, fixture.expected.contact.address);
    assert_eq!(contacts[0].name, fixture.expected.contact.name);
    assert_eq!(
        contacts[0].message_count,
        fixture.expected.contact.message_count
    );
    assert_eq!(
        contacts[0].last_contact_at,
        fixture.expected.contact.last_contact_at
    );
    assert_eq!(
        contacts[0].logo_key.as_deref(),
        Some(fixture.expected.contact.logo_key.as_str())
    );
    assert_eq!(list_labels(&fixture.messages), fixture.expected.labels);
    assert_eq!(
        build_notifications(&accounts, &fixture.messages, 30, &fixture.now)
            .into_iter()
            .map(|notification| notification.id)
            .collect::<Vec<_>>(),
        fixture.expected.notification_ids
    );
}

fn unique_root(prefix: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{timestamp}-{sequence}",
        std::process::id()
    ))
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(schema_version: u32) -> Self {
        let root = unique_root("imail-rust-storage");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("master.key"), "42".repeat(32)).unwrap();
        let connection = Connection::open(root.join("imail.sqlite")).unwrap();
        connection.execute_batch(&format!(r#"
          CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
          INSERT INTO metadata VALUES ('schema_version', '{schema_version}');
          CREATE TABLE accounts (id TEXT PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL, display_name TEXT NOT NULL, group_name TEXT NOT NULL, group_icon TEXT NOT NULL, color TEXT NOT NULL, settings_json TEXT NOT NULL, proxy_json TEXT, encrypted_secret TEXT NOT NULL, auth_method TEXT, created_at TEXT NOT NULL, last_sync_at TEXT, status TEXT NOT NULL, last_error TEXT, mailboxes_json TEXT NOT NULL, user_id TEXT NOT NULL);
          CREATE TABLE messages (id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id), mailbox TEXT NOT NULL, mailbox_role TEXT NOT NULL, uid INTEGER NOT NULL, message_id TEXT, from_json TEXT NOT NULL, to_json TEXT NOT NULL, subject TEXT NOT NULL, preview TEXT NOT NULL, text_body TEXT NOT NULL, html_body TEXT, received_at TEXT NOT NULL, unread INTEGER NOT NULL, flagged INTEGER NOT NULL, has_attachments INTEGER NOT NULL, attachments_json TEXT NOT NULL, labels_json TEXT NOT NULL, snoozed_until TEXT);
          CREATE TABLE drafts (id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id), to_json TEXT NOT NULL, cc_json TEXT NOT NULL, subject TEXT NOT NULL, text_body TEXT NOT NULL, html_body TEXT NOT NULL, attachments_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
          CREATE TABLE contacts (user_id TEXT NOT NULL, address TEXT NOT NULL COLLATE NOCASE, name TEXT NOT NULL, message_count INTEGER NOT NULL, last_contact_at TEXT NOT NULL, logo_key TEXT, logo_content_type TEXT, logo_source_url TEXT, logo_fetched_at TEXT, PRIMARY KEY (user_id, address));
          CREATE TABLE logo_fetch_attempts (user_id TEXT NOT NULL, target TEXT NOT NULL, domain_key TEXT NOT NULL, status TEXT NOT NULL, detail TEXT NOT NULL, attempted_at TEXT NOT NULL, PRIMARY KEY (user_id, target));
          CREATE TABLE developer_tokens (id TEXT PRIMARY KEY, user_id TEXT NOT NULL, name TEXT NOT NULL, token_hash TEXT NOT NULL, prefix TEXT NOT NULL, created_at TEXT NOT NULL, expires_at TEXT NOT NULL, last_used_at TEXT);
          CREATE TABLE developer_token_scopes (token_id TEXT NOT NULL, scope TEXT NOT NULL);
          CREATE TABLE developer_token_accounts (token_id TEXT NOT NULL, account_id TEXT NOT NULL);
          CREATE TABLE translation_provider_profiles (id TEXT PRIMARY KEY, user_id TEXT NOT NULL, display_name TEXT NOT NULL, execution_target TEXT NOT NULL, provider_json TEXT NOT NULL, credential_kind TEXT, encrypted_credential TEXT, enabled INTEGER NOT NULL, consent_revision TEXT, consent_accepted_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
          CREATE TABLE message_translation_cache (user_id TEXT NOT NULL, message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE, body_hash TEXT NOT NULL, source_language TEXT NOT NULL, target_language TEXT NOT NULL, profile_id TEXT NOT NULL, provider_revision TEXT NOT NULL, segment_version INTEGER NOT NULL, translated_segments_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY (user_id,message_id,body_hash,source_language,target_language,profile_id,provider_revision,segment_version));
          CREATE TABLE sync_policies (account_id TEXT PRIMARY KEY, enabled INTEGER NOT NULL, folder_mode TEXT NOT NULL, selected_mailboxes_json TEXT NOT NULL, notify_on_error INTEGER NOT NULL, updated_at TEXT NOT NULL);
          CREATE TABLE mailbox_sync_states (account_id TEXT NOT NULL, mailbox TEXT NOT NULL, mailbox_role TEXT NOT NULL, uid_validity TEXT, last_seen_uid INTEGER NOT NULL, highest_modseq TEXT, last_attempt_at TEXT, last_success_at TEXT, next_sync_at TEXT, consecutive_failures INTEGER NOT NULL, connection_status TEXT NOT NULL, sync_state TEXT NOT NULL, last_error_code TEXT, last_error_message TEXT);
          CREATE TABLE sync_jobs (id TEXT PRIMARY KEY, account_id TEXT NOT NULL, mailbox TEXT, mailbox_role TEXT NOT NULL, reason TEXT NOT NULL, status TEXT NOT NULL, priority INTEGER NOT NULL, not_before TEXT NOT NULL, locked_by TEXT, locked_until TEXT, attempts INTEGER NOT NULL, created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, synced_count INTEGER, new_count INTEGER, updated_count INTEGER, deleted_count INTEGER, error_code TEXT, error_message TEXT, rerun_requested INTEGER NOT NULL);
          INSERT INTO accounts VALUES ('a-later','gmail','Later@Example.com','Later','work','folder','#fff','{{"imapPort":993}}',NULL,'secret',NULL,'2026-02-01T00:00:00Z',NULL,'connected',NULL,'[]','user-1');
          INSERT INTO accounts VALUES ('a-first','outlook','First@Example.com','First','home','home','#000','{{"imapPort":993}}','{{"protocol":"socks5"}}','secret','oauth2','2026-01-01T00:00:00Z',NULL,'connected',NULL,'[]','user-1');
          INSERT INTO messages VALUES ('m1','a-first','INBOX','inbox',7,NULL,'{{"name":"Sender","address":"sender@example.com"}}','[]','Subject','Preview','Body',NULL,'2026-03-01T00:00:00Z',1,0,0,'[]','["important"]',NULL);
          INSERT INTO contacts VALUES ('user-1','Person@Example.com','Person',2,'2026-03-01T00:00:00Z',NULL,NULL,NULL,NULL);
          INSERT INTO developer_tokens VALUES ('t1','user-1','Agent','must-not-be-exposed','imail_','2026-01-01T00:00:00Z','2026-02-01T00:00:00Z',NULL);
          INSERT INTO developer_token_scopes VALUES ('t1','messages:read');
          INSERT INTO developer_token_accounts VALUES ('t1','a-first');
          INSERT INTO sync_policies VALUES ('a-first',1,'inbox','[]',1,'2026-01-01T00:00:00Z');
          INSERT INTO mailbox_sync_states VALUES ('a-first','INBOX','inbox','1',7,NULL,NULL,NULL,NULL,0,'connected','idle',NULL,NULL);
          INSERT INTO sync_jobs VALUES ('j1','a-first',NULL,'inbox','manual','queued',1,'2026-01-01T00:00:00Z',NULL,NULL,0,'2026-01-01T00:00:00Z',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,0);
        "#)).unwrap();
        drop(connection);
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn translation_cache_is_user_scoped_and_requires_message_ownership() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let key = TranslationCacheKey {
        user_id: "user-1".into(),
        message_id: "m1".into(),
        body_hash: "body-v1".into(),
        source_language: None,
        target_language: "zh-Hans".into(),
        profile_id: "edge-local".into(),
        provider_revision: "edge-local-v1".into(),
        segment_version: 1,
    };
    let artifact = TranslationArtifact {
        key: key.clone(),
        segments: vec![TranslatedSegment {
            id: "s-one".into(),
            text: "译文".into(),
        }],
        created_at: "2026-08-27T00:00:00.000Z".into(),
        updated_at: "2026-08-27T00:00:00.000Z".into(),
    };
    store.upsert_translation_artifact(&artifact).unwrap();
    assert_eq!(
        store.translation_artifact(&key).unwrap(),
        Some(artifact.clone())
    );

    let mut foreign_key = key.clone();
    foreign_key.user_id = "other-user".into();
    assert!(store.translation_artifact(&foreign_key).unwrap().is_none());
    let mut foreign_artifact = store.translation_artifact(&key).unwrap().unwrap();
    foreign_artifact.key = foreign_key;
    assert!(matches!(
        store.upsert_translation_artifact(&foreign_artifact),
        Err(AuthStoreError::TranslationCacheOwnershipViolation)
    ));

    assert_eq!(store.clear_translation_artifacts("other-user").unwrap(), 0);
    assert_eq!(store.clear_translation_artifacts("user-1").unwrap(), 1);
    assert!(store.translation_artifact(&key).unwrap().is_none());

    store.upsert_translation_artifact(&artifact).unwrap();
    assert!(store.delete_message("user-1", "m1").unwrap());
    assert!(store.translation_artifact(&key).unwrap().is_none());
}

#[test]
fn edge_client_completion_revalidates_segments_and_persists_owned_results() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user(
            "edge-owner@example.com",
            "Edge owner",
            "Edge owner password 123!",
        )
        .unwrap();
    store
        .connection
        .execute("UPDATE accounts SET user_id=?1", [&owner.id])
        .unwrap();
    TranslationSettingsService::new(&mut store)
        .upsert_profile(
            &owner.id,
            "edge-local",
            TranslationProviderProfileInput {
                display_name: "Edge 本地翻译".into(),
                execution_target: TranslationExecutionTarget::WebView,
                provider: TranslationProviderConfiguration::EdgeLocal,
                enabled: true,
            },
            "2026-08-27T00:00:00.000Z",
        )
        .unwrap();
    let artifact = TranslationService::new(&mut store)
        .complete(
            &owner.id,
            "m1",
            TranslationCompletionRequest {
                profile_id: "edge-local".into(),
                source_language: Some("en".into()),
                target_language: "zh-Hans".into(),
                segments: vec![TranslatedSegment {
                    id: "s-6ccaa6415b5ee449-1".into(),
                    text: "正文".into(),
                }],
            },
            "2026-08-27T00:00:00.000Z",
        )
        .unwrap();
    assert_eq!(artifact.segments[0].text, "正文");
    assert_eq!(
        store.translation_artifact(&artifact.key).unwrap(),
        Some(artifact)
    );
    let invalid = TranslationService::new(&mut store)
        .complete(
            &owner.id,
            "m1",
            TranslationCompletionRequest {
                profile_id: "edge-local".into(),
                source_language: Some("en".into()),
                target_language: "fr".into(),
                segments: vec![TranslatedSegment {
                    id: "client-invented-id".into(),
                    text: "Texte".into(),
                }],
            },
            "2026-08-27T00:00:00.000Z",
        )
        .unwrap_err();
    assert_eq!(invalid.code(), "TRANSLATION_RESULT_INVALID");
}

#[test]
fn message_pagination_uses_stable_composite_indexes_at_scale() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    migrate_database(&database_path).unwrap();
    let mut connection = Connection::open(&database_path).unwrap();
    let transaction = connection.transaction().unwrap();
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO messages (id, account_id, mailbox, mailbox_role, uid, message_id,
              from_json, to_json, subject, preview, text_body, html_body, received_at, unread,
              flagged, has_attachments, attachments_json, labels_json, snoozed_until)
             VALUES (?1, 'a-first', 'INBOX', 'inbox', ?2, NULL, ?3, '[]', ?4, ?5, '', NULL,
                     ?6, ?7, 0, 0, '[]', '[]', NULL)",
            )
            .unwrap();
        for index in 2..=100_000_u32 {
            let timestamp = format!(
                "2026-04-{:02}T{:02}:{:02}:{:02}Z",
                1 + index / 86_400,
                (index / 3_600) % 24,
                (index / 60) % 60,
                index % 60
            );
            insert
                .execute((
                    format!("bulk-{index:06}"),
                    i64::from(index),
                    r#"{"name":"Scale","address":"scale@example.test"}"#,
                    format!("Scale subject {index}"),
                    format!("Scale preview {index}"),
                    timestamp,
                    i64::from(index % 5 == 0),
                ))
                .unwrap();
        }
    }
    transaction.commit().unwrap();

    let plan = connection
        .prepare(
            "EXPLAIN QUERY PLAN SELECT m.id FROM messages m
         JOIN accounts a ON a.id=m.account_id
         WHERE a.user_id=?1 AND m.account_id=?2 AND m.mailbox_role='inbox' AND m.unread=1
           AND (m.received_at < ?3 OR (m.received_at = ?3 AND m.id < ?4))
         ORDER BY m.received_at DESC, m.id DESC LIMIT 61",
        )
        .unwrap()
        .query_map(
            ("user-1", "a-first", "2026-04-02T00:00:00Z", "bulk-086400"),
            |row| row.get::<_, String>(3),
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|detail| detail.contains("messages_account_role_unread_date_id")),
        "query plan did not use the pagination index: {plan:?}"
    );
    assert!(
        !plan
            .iter()
            .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
        "query plan sorts outside the index: {plan:?}"
    );

    drop(connection);
    let store = SqliteAuthStore::open_database(&database_path).unwrap();
    let started = std::time::Instant::now();
    let page = store
        .query_messages(
            "user-1",
            &MessageQuery {
                account_id: Some("a-first".into()),
                mailbox_role: Some("inbox".into()),
                unread: true,
                cursor: Some(GatewayMessageCursor {
                    date: "2026-04-02T00:00:00Z".into(),
                    id: "bulk-086400".into(),
                }),
                limit: 60,
                ..MessageQuery::default()
            },
            "2026-08-13T00:00:00Z",
        )
        .unwrap();
    assert_eq!(page.messages.len(), 60);
    assert!(page.has_more);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "100k-message cursor page exceeded the deterministic 2s ceiling"
    );

    let search_started = std::time::Instant::now();
    let search = store
        .query_messages(
            "user-1",
            &MessageQuery {
                text: Some("Scale subject 99999".into()),
                limit: 60,
                ..MessageQuery::default()
            },
            "2026-08-13T00:00:00Z",
        )
        .unwrap();
    assert_eq!(search.messages.len(), 1);
    assert_eq!(search.messages[0].subject, "Scale subject 99999");
    assert!(
        search_started.elapsed() < Duration::from_secs(2),
        "100k-message substring search exceeded the deterministic 2s ceiling; reassess FTS5"
    );
}

#[test]
fn reads_public_models_without_exposing_encrypted_values_or_token_hashes() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let before = fs::read(&database_path).unwrap();
    let store = SqliteReadOnlyStore::open_data_dir(&fixture.root).unwrap();
    let inventory = store.inventory().unwrap();
    let snapshot = store.read_snapshot().unwrap();
    assert_eq!(inventory.schema_version, 10);
    assert_eq!(inventory.table_counts["accounts"], 2);
    assert_eq!(
        snapshot
            .accounts
            .iter()
            .map(|account| account.id.as_str())
            .collect::<Vec<_>>(),
        ["a-first", "a-later"]
    );
    assert_eq!(snapshot.accounts[0].email, "First@Example.com");
    assert!(snapshot.messages[0].unread);
    assert!(!snapshot.messages[0].flagged);
    assert_eq!(snapshot.developer_tokens[0].scopes, ["messages:read"]);
    let serialized = serde_json::to_string(&snapshot).unwrap();
    assert!(!serialized.contains("must-not-be-exposed"));
    assert!(!serialized.contains("encryptedSecret"));
    drop(store);
    assert_eq!(fs::read(database_path).unwrap(), before);
}

#[test]
fn rejects_future_schema_without_writing_to_the_database() {
    let fixture = Fixture::new(999);
    let database_path = fixture.root.join("imail.sqlite");
    let before = fs::read(&database_path).unwrap();
    let error = SqliteReadOnlyStore::open_data_dir(&fixture.root)
        .err()
        .unwrap();
    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            actual: 999,
            supported: 10
        }
    ));
    assert_eq!(fs::read(database_path).unwrap(), before);
}

#[test]
fn requires_a_valid_master_key_for_data_directory_inspection() {
    let fixture = Fixture::new(10);
    fs::remove_file(fixture.root.join("master.key")).unwrap();
    assert!(matches!(
        SqliteReadOnlyStore::open_data_dir(&fixture.root).err(),
        Some(StorageError::InvalidMasterKey)
    ));
}

#[test]
fn reads_an_older_compatible_schema_without_migrating_it() {
    let fixture = Fixture::new(5);
    let database_path = fixture.root.join("imail.sqlite");
    let before = fs::read(&database_path).unwrap();
    let store = SqliteReadOnlyStore::open_data_dir(&fixture.root).unwrap();
    assert_eq!(store.inventory().unwrap().schema_version, 5);
    assert_eq!(store.read_snapshot().unwrap().accounts.len(), 2);
    drop(store);
    assert_eq!(fs::read(database_path).unwrap(), before);
}

#[test]
fn rejects_corrupt_sqlite_without_replacing_or_truncating_it() {
    let root = unique_root("imail-rust-corrupt");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("master.key"), "42".repeat(32)).unwrap();
    let database_path = root.join("imail.sqlite");
    let corrupt = b"not a sqlite database".to_vec();
    fs::write(&database_path, &corrupt).unwrap();
    assert!(SqliteReadOnlyStore::open_data_dir(&root).is_err());
    assert_eq!(fs::read(&database_path).unwrap(), corrupt);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_malformed_json_without_mutation() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute(
            "UPDATE accounts SET settings_json = 'not-json' WHERE id = 'a-first'",
            [],
        )
        .unwrap();
    drop(connection);
    let before = fs::read(&database_path).unwrap();
    let store = SqliteReadOnlyStore::open_data_dir(&fixture.root).unwrap();
    assert!(store.read_snapshot().is_err());
    drop(store);
    assert_eq!(fs::read(database_path).unwrap(), before);
}

#[test]
fn rejects_non_boolean_integer_without_mutation() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE messages SET unread = 2 WHERE id = 'm1'", [])
        .unwrap();
    drop(connection);
    let before = fs::read(&database_path).unwrap();
    let store = SqliteReadOnlyStore::open_data_dir(&fixture.root).unwrap();
    assert!(store.read_snapshot().is_err());
    drop(store);
    assert_eq!(fs::read(database_path).unwrap(), before);
}

#[test]
fn rust_auth_transactions_claim_legacy_rows_and_persist_only_session_hashes() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id = '__legacy__'", [])
        .unwrap();
    connection
        .execute(
            "INSERT INTO metadata (key, value) VALUES ('app_preferences_v1', '{}')",
            [],
        )
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    assert!(store.setup_required().unwrap());
    let user = store
        .create_user("FIRST@Example.com", "First User", "Correct horse 123!")
        .unwrap();
    assert_eq!(user.login, "first@example.com");
    assert!(store
        .authenticate("first@example.COM", "Correct horse 123!")
        .unwrap()
        .is_some());
    assert!(store
        .authenticate("first@example.com", "wrong")
        .unwrap()
        .is_none());
    let raw_session = store.create_session(&user.id).unwrap();
    assert_eq!(raw_session.len(), 43);
    let observer = Connection::open(&database_path).unwrap();
    let stored_hash: String = observer
        .query_row("SELECT token_hash FROM app_sessions LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored_hash, sha256_hex(&raw_session));
    assert_ne!(stored_hash, raw_session);
    drop(observer);
    assert_eq!(
        store.user_for_session(&raw_session).unwrap(),
        Some(user.clone())
    );
    store.delete_session(&raw_session).unwrap();
    assert!(store.user_for_session(&raw_session).unwrap().is_none());
    let rate_key = "login:192.0.2.1:first@example.com";
    assert!(store.consume_attempt(rate_key, 2, 60_000).unwrap().allowed);
    assert!(store.consume_attempt(rate_key, 2, 60_000).unwrap().allowed);
    let rejected = store.consume_attempt(rate_key, 2, 60_000).unwrap();
    assert!(!rejected.allowed);
    assert!((1..=60).contains(&rejected.retry_after));
    store.clear_attempt(rate_key).unwrap();
    assert!(store.consume_attempt(rate_key, 2, 60_000).unwrap().allowed);
    let observer = Connection::open(&database_path).unwrap();
    let stored_rate_key: String = observer
        .query_row("SELECT key_hash FROM auth_rate_limits LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored_rate_key, sha256_hex(rate_key));
    assert_ne!(stored_rate_key, rate_key);
    drop(observer);
    drop(store);

    let connection = Connection::open(&database_path).unwrap();
    let owner: String = connection
        .query_row("SELECT user_id FROM accounts LIMIT 1", [], |row| row.get(0))
        .unwrap();
    let preference: String = connection
        .query_row(
            "SELECT key FROM metadata WHERE value = '{}' AND key LIKE 'user:%:app_preferences_v1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let session_count: i64 = connection
        .query_row("SELECT count(*) FROM app_sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(owner, user.id);
    assert_eq!(preference, format!("user:{}:app_preferences_v1", user.id));
    assert_eq!(session_count, 0);
}

#[test]
fn rust_developer_tokens_enforce_owner_scope_and_hash_boundaries() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id = '__legacy__'", [])
        .unwrap();
    drop(connection);

    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("owner@example.com", "Owner", "Owner password 123!")
        .unwrap();
    let outsider = store
        .create_user("other@example.com", "Other", "Other password 123!")
        .unwrap();
    let issued = store
        .issue_developer_token(
            &owner.id,
            "Read token",
            &["messages:read".into()],
            &["a-first".into()],
            3_600,
        )
        .unwrap();
    assert!(issued.raw.starts_with("imail_"));
    assert!(!issued.raw.starts_with("imail_mcp_"));
    assert_eq!(issued.raw.len(), 46);
    assert_eq!(issued.token.prefix, issued.raw[..12]);
    assert_eq!(issued.token.account_ids, ["a-first"]);
    assert!(store
        .authenticate_developer_token(&issued.raw, "messages:read")
        .unwrap()
        .is_some());
    assert!(store
        .authenticate_developer_token(&issued.raw, "messages:send")
        .unwrap()
        .is_none());
    assert!(matches!(
        store.issue_developer_token(
            &outsider.id,
            "Cross-user token",
            &["messages:read".into()],
            &["a-first".into()],
            3_600,
        ),
        Err(AuthStoreError::InvalidTokenAccounts)
    ));

    let mcp = store
        .issue_developer_token(&owner.id, "MCP token", &["mcp:full".into()], &[], 3_600)
        .unwrap();
    assert!(mcp.raw.starts_with("imail_mcp_"));
    assert_eq!(mcp.raw.len(), 50);
    assert_eq!(mcp.token.account_ids, ["a-first", "a-later"]);
    assert_eq!(store.list_developer_tokens(&owner.id).unwrap().len(), 2);
    assert!(store
        .list_developer_tokens(&outsider.id)
        .unwrap()
        .is_empty());
    assert!(!store
        .revoke_developer_token(&outsider.id, &issued.token.id)
        .unwrap());
    assert!(store
        .authenticate_developer_token(&issued.raw, "messages:read")
        .unwrap()
        .is_some());
    assert!(store
        .revoke_developer_token(&owner.id, &issued.token.id)
        .unwrap());
    assert!(store
        .authenticate_developer_token(&issued.raw, "messages:read")
        .unwrap()
        .is_none());
    drop(store);

    let connection = Connection::open(&database_path).unwrap();
    let stored_hash: String = connection
        .query_row(
            "SELECT token_hash FROM developer_tokens WHERE id = ?1",
            [&mcp.token.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_hash, sha256_hex(&mcp.raw));
    assert_ne!(stored_hash, mcp.raw);
    let revoked_children: i64 = connection
        .query_row(
            "SELECT (SELECT count(*) FROM developer_token_scopes WHERE token_id = ?1)
                    + (SELECT count(*) FROM developer_token_accounts WHERE token_id = ?1)",
            [&issued.token.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(revoked_children, 0);
}

#[test]
fn rust_account_and_metadata_writes_are_user_scoped_and_node_compatible() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("account-owner@example.com", "Owner", "Owner password 123!")
        .unwrap();
    let outsider = store
        .create_user("account-other@example.com", "Other", "Other password 123!")
        .unwrap();
    let key = MasterKey::from_hex(&"42".repeat(32)).unwrap();
    let encrypted_secret = key
        .encrypt_json(&json!({ "authType": "app-password", "password": "temporary" }))
        .unwrap();
    let mut account = AccountRecord {
        id: "rust-account".into(),
        owner_id: owner.id.clone(),
        provider: "custom".into(),
        email: "RUST.Account@Example.com".into(),
        display_name: "Rust Account".into(),
        group: "Tests".into(),
        group_icon: "code".into(),
        color: "#168f78".into(),
        settings: json!({
            "imapHost": "imap.example.com", "imapPort": 993, "imapSecure": true,
            "smtpHost": "smtp.example.com", "smtpPort": 465, "smtpSecure": true
        }),
        proxy: Some(json!({ "protocol": "socks5", "host": "127.0.0.1", "port": 1080 })),
        encrypted_secret,
        auth_method: Some("app-password".into()),
        created_at: "2026-08-10T00:00:00.000Z".into(),
        last_sync_at: None,
        status: "connected".into(),
        last_error: None,
        mailboxes: json!([]),
    };
    store.upsert_account(&account).unwrap();
    let stored = store.account(&owner.id, &account.id).unwrap().unwrap();
    assert_eq!(stored.email, "rust.account@example.com");
    assert_eq!(stored.display_name, "Rust Account");
    assert_eq!(
        key.decrypt_json::<serde_json::Value>(&stored.encrypted_secret)
            .unwrap(),
        json!({ "authType": "app-password", "password": "temporary" })
    );
    assert!(store.account(&outsider.id, &account.id).unwrap().is_none());

    account.display_name = "Updated Name".into();
    account.created_at = "2099-01-01T00:00:00.000Z".into();
    store.upsert_account(&account).unwrap();
    let updated = store.account(&owner.id, &account.id).unwrap().unwrap();
    assert_eq!(updated.display_name, "Updated Name");
    assert_eq!(updated.created_at, "2026-08-10T00:00:00.000Z");

    let mut stolen = account.clone();
    stolen.owner_id = outsider.id.clone();
    assert!(matches!(
        store.upsert_account(&stolen),
        Err(AuthStoreError::AccountOwnershipViolation)
    ));
    let mut duplicate = account.clone();
    duplicate.id = "duplicate-account".into();
    assert!(matches!(
        store.upsert_account(&duplicate),
        Err(AuthStoreError::DuplicateAccount)
    ));
    assert!(store
        .account(&owner.id, "duplicate-account")
        .unwrap()
        .is_none());

    store
        .set_user_metadata(
            &owner.id,
            "app_preferences_v1",
            r#"{"defaultMessageView":"rendered"}"#,
        )
        .unwrap();
    assert_eq!(
        store
            .user_metadata(&owner.id, "app_preferences_v1")
            .unwrap()
            .as_deref(),
        Some(r#"{"defaultMessageView":"rendered"}"#)
    );
    assert!(store
        .user_metadata(&outsider.id, "app_preferences_v1")
        .unwrap()
        .is_none());
    assert!(matches!(
        store.set_user_metadata(&owner.id, "../schema_version", "7"),
        Err(AuthStoreError::InvalidMetadataKey)
    ));
    assert!(!store.delete_account(&outsider.id, &account.id).unwrap());
    assert!(store.delete_account(&owner.id, &account.id).unwrap());
    assert!(store.account(&owner.id, &account.id).unwrap().is_none());
}

#[test]
fn apple_hme_sessions_are_owner_scoped_encrypted_and_removed_with_the_account() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    migrate_database(&database_path).unwrap();
    let key = MasterKey::from_hex(&"42".repeat(32)).unwrap();
    let encrypted = key
        .encrypt_json(&json!({
            "appleId": "owner@icloud.com",
            "loginStates": [{"kind": "i_cloud_web", "cookie": "must-not-leak"}]
        }))
        .unwrap();
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let saved = store
        .upsert_apple_hme_session("user-1", "a-first", &encrypted)
        .unwrap();
    assert_eq!(saved.account_id, "a-first");
    assert_eq!(saved.user_id, "user-1");
    assert!(!saved.updated_at.is_empty());
    assert_eq!(
        store
            .apple_hme_session("user-1", "a-first")
            .unwrap()
            .unwrap()
            .encrypted_session,
        encrypted
    );
    assert!(store
        .apple_hme_session("other-user", "a-first")
        .unwrap()
        .is_none());
    assert!(matches!(
        store.upsert_apple_hme_session("other-user", "a-first", &encrypted),
        Err(AuthStoreError::AccountNotOwned)
    ));
    assert!(!fs::read(&database_path)
        .unwrap()
        .windows("must-not-leak".len())
        .any(|window| window == b"must-not-leak"));

    assert!(store.delete_account("user-1", "a-first").unwrap());
    assert!(store
        .apple_hme_session("user-1", "a-first")
        .unwrap()
        .is_none());
}

#[test]
fn apple_hme_addresses_are_replaced_mutated_and_persisted_by_owner() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    migrate_database(&database_path).unwrap();
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let address = AppleHmeAddressRecord {
        account_id: "a-first".into(),
        user_id: "user-1".into(),
        anonymous_id: "hme-1".into(),
        email: "quiet-path@icloud.com".into(),
        label: "Shopping".into(),
        note: "Local snapshot".into(),
        forward_to_email: "first@example.com".into(),
        active: true,
        origin: "icloud-web".into(),
        created_at: Some("2026-08-18T00:00:00Z".into()),
        updated_at: String::new(),
    };
    let snapshot = store
        .replace_apple_hme_addresses("user-1", "a-first", std::slice::from_ref(&address))
        .unwrap();
    assert_eq!(snapshot.addresses.len(), 1);
    assert!(snapshot.last_synced_at.is_some());
    assert_eq!(snapshot.addresses[0].email, "quiet-path@icloud.com");
    assert!(matches!(
        store.apple_hme_addresses("other-user", "a-first"),
        Err(AuthStoreError::AccountNotOwned)
    ));

    assert!(store
        .set_apple_hme_address_active("user-1", "a-first", "hme-1", false)
        .unwrap());
    drop(store);
    let mut reopened = SqliteAuthStore::open_database(&database_path).unwrap();
    let persisted = reopened.apple_hme_addresses("user-1", "a-first").unwrap();
    assert!(!persisted.addresses[0].active);
    assert!(reopened
        .delete_apple_hme_address("user-1", "a-first", "hme-1")
        .unwrap());
    assert!(reopened
        .apple_hme_addresses("user-1", "a-first")
        .unwrap()
        .addresses
        .is_empty());

    reopened
        .replace_apple_hme_addresses("user-1", "a-first", &[address])
        .unwrap();
    assert!(reopened.delete_account("user-1", "a-first").unwrap());
    let connection = Connection::open(&database_path).unwrap();
    let remaining: i64 = connection
        .query_row(
            "SELECT count(*) FROM apple_hme_addresses WHERE account_id='a-first'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
}

#[test]
fn rust_write_transaction_rolls_back_parent_when_relation_insert_fails() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let user = store
        .create_user("rollback@example.com", "Rollback", "Rollback password 123!")
        .unwrap();
    let observer = Connection::open(&database_path).unwrap();
    observer
        .execute_batch(
            "CREATE TRIGGER reject_contract_scope BEFORE INSERT ON developer_token_scopes
             BEGIN SELECT RAISE(ABORT, 'intentional contract rollback'); END;",
        )
        .unwrap();
    drop(observer);
    assert!(store
        .issue_developer_token(&user.id, "Must roll back", &["mcp:full".into()], &[], 3_600,)
        .is_err());
    drop(store);
    let observer = Connection::open(&database_path).unwrap();
    let partial_rows: i64 = observer
        .query_row(
            "SELECT count(*) FROM developer_tokens WHERE user_id = ?1",
            [&user.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(partial_rows, 0);
}

#[test]
fn rust_writer_waits_for_short_sqlite_lock_and_then_commits() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let user = store
        .create_user("lock@example.com", "Lock", "Lock password 123!")
        .unwrap();
    let blocker = Connection::open(&database_path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let user_id = user.id.clone();
    let writer = std::thread::spawn(move || {
        store.set_user_metadata(&user_id, "app_preferences_v1", r#"{"theme":"tech"}"#)
    });
    std::thread::sleep(Duration::from_millis(100));
    blocker.execute_batch("COMMIT").unwrap();
    writer.join().unwrap().unwrap();
    let observer = Connection::open(&database_path).unwrap();
    let value: String = observer
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [format!("user:{}:app_preferences_v1", user.id)],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, r#"{"theme":"tech"}"#);
}

#[test]
fn rust_preferences_service_matches_node_defaults_merging_and_validation() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user(
            "preferences@example.com",
            "Preferences",
            "Preferences password 123!",
        )
        .unwrap();
    let outsider = store
        .create_user(
            "preferences-other@example.com",
            "Other",
            "Other password 123!",
        )
        .unwrap();

    {
        let mut service = PreferencesService::new(&mut store);
        assert_eq!(service.read(&owner.id).unwrap(), AppPreferences::default());
        let updated = service
            .update(
                &owner.id,
                AppPreferencesPatch {
                    theme: Some(ThemeId::Tech),
                    default_message_view: Some(MessageView::Rendered),
                    notification_kinds: Some(NotificationKindsPatch {
                        snooze: Some(false),
                        ..Default::default()
                    }),
                    shortcut_bindings: Some(ShortcutBindingsPatch {
                        focus_search: Some("Mod+Shift+K".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.theme, ThemeId::Tech);
        assert_eq!(updated.default_message_view, MessageView::Rendered);
        assert!(updated.notification_kinds.unread);
        assert!(!updated.notification_kinds.snooze);
        assert_eq!(updated.shortcut_bindings.focus_search, "Mod+Shift+K");
        assert_eq!(
            service.read(&outsider.id).unwrap(),
            AppPreferences::default()
        );

        let empty = service
            .update(&owner.id, AppPreferencesPatch::default())
            .unwrap_err();
        assert_eq!(empty.code(), "PREFERENCES_UPDATE_EMPTY");
        assert_eq!(empty.status(), 400);
        let too_long = service
            .update(
                &owner.id,
                AppPreferencesPatch {
                    shortcut_bindings: Some(ShortcutBindingsPatch {
                        focus_search: Some("😀".repeat(31)),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert_eq!(too_long.code(), "PREFERENCES_SHORTCUT_INVALID");
    }

    store
        .set_user_metadata(&owner.id, "app_preferences_v1", "not-json")
        .unwrap();
    assert_eq!(
        PreferencesService::new(&mut store).read(&owner.id).unwrap(),
        AppPreferences::default()
    );
}

#[test]
fn rust_draft_service_preserves_creation_time_and_enforces_ownership() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id='__legacy__'", [])
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("draft-owner@example.com", "Owner", "Owner password 123!")
        .unwrap();
    let outsider = store
        .create_user("draft-other@example.com", "Other", "Other password 123!")
        .unwrap();
    let input = |subject: &str| DraftInput {
        account_id: "a-first".into(),
        to: json!([{ "address": "recipient@example.com" }]),
        cc: json!([]),
        subject: subject.into(),
        text: "Body".into(),
        html: "<p>Body</p>".into(),
        attachments: json!([]),
    };

    let mut service = DraftService::new(&mut store);
    let created = service
        .create(
            &owner.id,
            "draft-rust",
            "2026-08-10T01:00:00.000Z",
            input("First"),
        )
        .unwrap();
    assert_eq!(created.created_at, "2026-08-10T01:00:00.000Z");
    let replaced = service
        .create(
            &owner.id,
            "draft-rust",
            "2026-08-10T02:00:00.000Z",
            input("Replaced"),
        )
        .unwrap();
    assert_eq!(replaced.created_at, created.created_at);
    assert_eq!(replaced.updated_at, "2026-08-10T02:00:00.000Z");
    assert_eq!(replaced.subject, "Replaced");
    assert_eq!(service.get(&owner.id, "draft-rust").unwrap(), replaced);
    assert_eq!(
        service.get(&outsider.id, "draft-rust").unwrap_err().code(),
        "DRAFT_NOT_FOUND"
    );
    assert!(!service.delete(&outsider.id, "draft-rust").unwrap());
    assert_eq!(
        service
            .save_existing(
                &owner.id,
                "missing",
                "2026-08-10T03:00:00.000Z",
                input("Missing"),
            )
            .unwrap_err()
            .code(),
        "DRAFT_NOT_FOUND"
    );
    let mut foreign_input = input("Foreign");
    foreign_input.account_id = "missing-account".into();
    assert_eq!(
        service
            .create(
                &owner.id,
                "foreign",
                "2026-08-10T03:00:00.000Z",
                foreign_input,
            )
            .unwrap_err()
            .code(),
        "ACCOUNT_NOT_FOUND"
    );
    assert!(service.delete(&owner.id, "draft-rust").unwrap());
    assert!(service.list(&owner.id).unwrap().is_empty());
}

#[test]
fn rust_contacts_service_reconciles_and_reuses_root_logo_without_automatic_retry() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id='__legacy__'", [])
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("contacts@example.com", "Contacts", "Contacts password 123!")
        .unwrap();

    let root_logo = ContactReadModel {
        owner_id: owner.id.clone(),
        address: "logo@example.com".into(),
        name: "Logo source".into(),
        message_count: 1,
        last_contact_at: "2026-01-01T00:00:00.000Z".into(),
        logo_key: Some("domain:example.com".into()),
        logo_content_type: Some("image/png".into()),
        logo_source_url: Some("https://example.com/favicon.png".into()),
        logo_fetched_at: Some("2026-01-01T00:00:00.000Z".into()),
    };
    store.upsert_contact(&root_logo).unwrap();
    let original = store
        .list_messages(&owner.id)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let mut older = original.clone();
    older.id = "contact-older".into();
    older.uid = 100;
    older.from = json!({ "name": "Support Old", "address": "support@mail.example.com" });
    older.to = json!([
        { "name": "Duplicate", "address": "SUPPORT@mail.example.com" },
        { "name": "Own", "address": "First@Example.com" }
    ]);
    older.date = "2026-08-10T01:00:00.000Z".into();
    store.upsert_message(&owner.id, &older).unwrap();
    let mut newer = older.clone();
    newer.id = "contact-newer".into();
    newer.uid = 101;
    newer.from = json!({ "name": "Support New", "address": "Support@mail.example.com" });
    newer.to = json!([]);
    newer.date = "2026-08-10T02:00:00.000Z".into();
    store.upsert_message(&owner.id, &newer).unwrap();
    let attempt = LogoFetchAttemptRecord {
        owner_id: owner.id.clone(),
        target: "https://mail.example.com".into(),
        domain_key: "domain:mail.example.com".into(),
        status: "failed".into(),
        detail: "not found".into(),
        attempted_at: "2026-08-10T02:00:00.000Z".into(),
    };
    store.upsert_logo_fetch_attempt(&attempt).unwrap();

    let domains = TestDomains;
    let mut service = ContactsService::new(&mut store, &domains);
    assert!(!service
        .should_attempt_logo(&owner.id, "HTTPS://MAIL.EXAMPLE.COM")
        .unwrap());
    assert!(service
        .should_attempt_logo(&owner.id, "https://new.example.com")
        .unwrap());
    let contacts = service.reconcile(&owner.id).unwrap();
    let support = contacts
        .iter()
        .find(|contact| {
            contact
                .address
                .eq_ignore_ascii_case("support@mail.example.com")
        })
        .unwrap();
    assert_eq!(support.name, "Support New");
    assert_eq!(support.message_count, 2);
    assert_eq!(support.logo_key.as_deref(), Some("domain:example.com"));
    assert!(!contacts
        .iter()
        .any(|contact| contact.address.eq_ignore_ascii_case("first@example.com")));
    assert!(!contacts_need_logo_update(
        &contacts,
        &support.address,
        support,
        &domains
    ));
    let mut changed_logo = support.clone();
    changed_logo.logo_source_url = Some("https://example.com/new.png".into());
    assert!(contacts_need_logo_update(
        &contacts,
        &support.address,
        &changed_logo,
        &domains
    ));
    let stored_support = store
        .list_contacts(&owner.id)
        .unwrap()
        .into_iter()
        .find(|contact| {
            contact
                .address
                .eq_ignore_ascii_case("support@mail.example.com")
        })
        .unwrap();
    assert!(stored_support
        .address
        .eq_ignore_ascii_case(&support.address));
    assert_eq!(stored_support.name, support.name);
    assert_eq!(stored_support.message_count, support.message_count);
    assert_eq!(stored_support.logo_key, support.logo_key);
}

#[test]
fn rust_account_service_updates_only_owned_metadata_and_returns_safe_views() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id='__legacy__'", [])
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("metadata-owner@example.com", "Owner", "Owner password 123!")
        .unwrap();
    let outsider = store
        .create_user("metadata-other@example.com", "Other", "Other password 123!")
        .unwrap();
    let secret_before = store
        .account(&owner.id, "a-first")
        .unwrap()
        .unwrap()
        .encrypted_secret;

    let mut service = AccountService::new(&mut store);
    assert_eq!(service.list(&owner.id).unwrap().len(), 2);
    let updated = service
        .update_metadata(
            &owner.id,
            "a-first",
            AccountMetadataPatch {
                display_name: Some("  Primary inbox  ".into()),
                group: Some("  Work  ".into()),
                group_icon: Some(WorkspaceIconId::Briefcase),
                color: Some("#12aBcF".into()),
            },
        )
        .unwrap();
    assert_eq!(updated.display_name, "Primary inbox");
    assert_eq!(updated.group, "Work");
    assert_eq!(updated.group_icon, "briefcase");
    assert_eq!(updated.color, "#12aBcF");
    let serialized = serde_json::to_string(&updated).unwrap();
    assert!(!serialized.contains("encrypted"));
    assert!(!serialized.contains(&secret_before));
    assert_eq!(
        service.get(&outsider.id, "a-first").unwrap_err().code(),
        "ACCOUNT_NOT_FOUND"
    );
    assert_eq!(
        service
            .update_metadata(&owner.id, "a-first", AccountMetadataPatch::default())
            .unwrap_err()
            .code(),
        "ACCOUNT_METADATA_EMPTY"
    );
    assert_eq!(
        service
            .update_metadata(
                &owner.id,
                "a-first",
                AccountMetadataPatch {
                    color: Some("red".into()),
                    ..Default::default()
                },
            )
            .unwrap_err()
            .code(),
        "ACCOUNT_METADATA_INVALID"
    );
    assert_eq!(
        store
            .account(&owner.id, "a-first")
            .unwrap()
            .unwrap()
            .encrypted_secret,
        secret_before
    );
}

#[test]
fn rust_account_proxy_and_password_updates_validate_before_persisting_secrets() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id='__legacy__'", [])
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("proxy-owner@example.com", "Owner", "Owner password 123!")
        .unwrap();
    let key = MasterKey::from_hex(&"42".repeat(32)).unwrap();
    let codec = MasterKeyCredentialCodec::new(&key);
    let mut target = store.account(&owner.id, "a-first").unwrap().unwrap();
    target.auth_method = Some("app-password".into());
    target.encrypted_secret = key
        .encrypt_json(&json!({
            "authType": "app-password",
            "password": "target-mail-password",
            "proxyPassword": "target-proxy-password"
        }))
        .unwrap();
    target.proxy = Some(json!({
        "protocol": "http", "host": "old-proxy.example.com", "port": 8080
    }));
    store.upsert_account(&target).unwrap();
    let mut source = store.account(&owner.id, "a-later").unwrap().unwrap();
    source.auth_method = Some("app-password".into());
    source.encrypted_secret = key
        .encrypt_json(&json!({
            "authType": "app-password",
            "password": "source-mail-password",
            "proxyPassword": "source-proxy-password"
        }))
        .unwrap();
    source.proxy = Some(json!({
        "protocol": "socks5", "host": "proxy.example.com", "port": 1080,
        "username": "proxy-user"
    }));
    store.upsert_account(&source).unwrap();

    let validation_calls = Cell::new(0);
    let validator = |candidate: &AccountRecord| {
        validation_calls.set(validation_calls.get() + 1);
        assert_eq!(candidate.status, "syncing");
        assert!(candidate.last_error.is_none());
        Ok::<(), CredentialValidationError>(())
    };
    let copied = AccountService::new(&mut store)
        .update_proxy(
            &owner.id,
            "a-first",
            AccountProxyUpdate::CopyFrom {
                source_account_id: "a-later".into(),
            },
            &codec,
            &validator,
        )
        .unwrap();
    assert_eq!(copied.proxy, source.proxy);
    assert_eq!(copied.status, "connected");
    assert_eq!(validation_calls.get(), 1);
    let copied_record = store.account(&owner.id, "a-first").unwrap().unwrap();
    let copied_secret: serde_json::Value =
        key.decrypt_json(&copied_record.encrypted_secret).unwrap();
    assert_eq!(copied_secret["password"], "target-mail-password");
    assert_eq!(copied_secret["proxyPassword"], "source-proxy-password");

    let before_failure = copied_record.encrypted_secret.clone();
    let rejected = |_candidate: &AccountRecord| Err(CredentialValidationError);
    assert_eq!(
        AccountService::new(&mut store)
            .update_proxy(
                &owner.id,
                "a-first",
                AccountProxyUpdate::Explicit {
                    protocol: ProxyProtocol::Https,
                    host: "rejected.example.com".into(),
                    port: 8443,
                    username: None,
                    password: Some("must-not-persist".into()),
                },
                &codec,
                &rejected,
            )
            .unwrap_err()
            .code(),
        "ACCOUNT_CONNECTION_FAILED"
    );
    assert_eq!(
        store
            .account(&owner.id, "a-first")
            .unwrap()
            .unwrap()
            .encrypted_secret,
        before_failure
    );

    AccountService::new(&mut store)
        .update_proxy(
            &owner.id,
            "a-first",
            AccountProxyUpdate::Explicit {
                protocol: ProxyProtocol::Http,
                host: " proxy-two.example.com ".into(),
                port: 3128,
                username: Some("  proxy-user-two  ".into()),
                password: Some("new-proxy-password".into()),
            },
            &codec,
            &validator,
        )
        .unwrap();
    AccountService::new(&mut store)
        .replace_password(
            &owner.id,
            "a-first",
            "new-mail-password",
            &codec,
            &validator,
        )
        .unwrap();
    let password_record = store.account(&owner.id, "a-first").unwrap().unwrap();
    let password_secret: serde_json::Value =
        key.decrypt_json(&password_record.encrypted_secret).unwrap();
    assert_eq!(password_secret["password"], "new-mail-password");
    assert_eq!(password_secret["proxyPassword"], "new-proxy-password");
    assert!(password_secret.get("accessToken").is_none());

    AccountService::new(&mut store)
        .update_proxy(
            &owner.id,
            "a-first",
            AccountProxyUpdate::Disabled,
            &codec,
            &validator,
        )
        .unwrap();
    let disabled = store.account(&owner.id, "a-first").unwrap().unwrap();
    assert!(disabled.proxy.is_none());
    assert!(key
        .decrypt_json::<serde_json::Value>(&disabled.encrypted_secret)
        .unwrap()
        .get("proxyPassword")
        .is_none());

    assert_eq!(
        AccountService::new(&mut store)
            .update_proxy(
                &owner.id,
                "a-first",
                AccountProxyUpdate::CopyFrom {
                    source_account_id: "a-first".into(),
                },
                &codec,
                &validator,
            )
            .unwrap_err()
            .code(),
        "PROXY_SOURCE_SAME_ACCOUNT"
    );
}

#[test]
fn rust_account_removal_cascades_owned_content_without_revoking_the_whole_token() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id='__legacy__'", [])
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("remove-owner@example.com", "Owner", "Owner password 123!")
        .unwrap();
    store
        .upsert_draft(
            &owner.id,
            &DraftReadModel {
                id: "remove-draft".into(),
                account_id: "a-first".into(),
                to: json!([]),
                cc: json!([]),
                subject: "Remove".into(),
                text: String::new(),
                html: String::new(),
                attachments: json!([]),
                created_at: "2026-08-10T00:00:00.000Z".into(),
                updated_at: "2026-08-10T00:00:00.000Z".into(),
            },
        )
        .unwrap();
    let token = store
        .issue_developer_token(
            &owner.id,
            "Both accounts",
            &["messages:read".into()],
            &["a-first".into(), "a-later".into()],
            3_600,
        )
        .unwrap();
    let removed = AccountService::new(&mut store)
        .remove(&owner.id, "a-first")
        .unwrap();
    assert_eq!(removed.id, "a-first");
    assert!(store.account(&owner.id, "a-first").unwrap().is_none());
    assert!(store.list_messages(&owner.id).unwrap().is_empty());
    assert!(store.list_drafts(&owner.id).unwrap().is_empty());
    assert!(store.account(&owner.id, "a-later").unwrap().is_some());
    let remaining_token = store
        .list_developer_tokens(&owner.id)
        .unwrap()
        .into_iter()
        .find(|item| item.id == token.token.id)
        .unwrap();
    assert_eq!(remaining_token.account_ids, ["a-later"]);
    let observer = Connection::open(&database_path).unwrap();
    for table in [
        "sync_policies",
        "mailbox_sync_states",
        "sync_jobs",
        "developer_token_accounts",
    ] {
        let count: i64 = observer
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE account_id='a-first'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "orphan rows remained in {table}");
    }
    assert_eq!(
        AccountService::new(&mut store)
            .remove(&owner.id, "a-first")
            .unwrap_err()
            .code(),
        "ACCOUNT_NOT_FOUND"
    );
}

#[test]
fn rust_authorization_export_is_node_compatible_and_whitelists_sensitive_fields() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id='__legacy__'", [])
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("export-owner@example.com", "Owner", "Owner password 123!")
        .unwrap();
    let outsider = store
        .create_user("export-other@example.com", "Other", "Other password 123!")
        .unwrap();
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute(
            "UPDATE accounts SET user_id=?1 WHERE id='a-later'",
            [&outsider.id],
        )
        .unwrap();
    drop(connection);
    let key = MasterKey::from_hex(&"42".repeat(32)).unwrap();
    let codec = MasterKeyCredentialCodec::new(&key);
    let mut account = store.account(&owner.id, "a-first").unwrap().unwrap();
    account.provider = "gmail".into();
    account.email = "owner@example.com".into();
    account.display_name = "Owner".into();
    account.group = "个人".into();
    account.group_icon = "users".into();
    account.color = "#168f78".into();
    account.auth_method = Some("oauth2".into());
    account.settings = json!({
        "imapHost": "imap.gmail.com", "imapPort": 993, "imapSecure": true,
        "smtpHost": "smtp.gmail.com", "smtpPort": 465, "smtpSecure": true
    });
    account.proxy = Some(json!({
        "protocol": "https", "host": "proxy.example.com", "port": 8443,
        "username": "proxy-user"
    }));
    account.encrypted_secret = key
        .encrypt_json(&json!({
            "authType": "oauth2", "oauthProvider": "google",
            "accessToken": "access-secret", "refreshToken": "refresh-secret",
            "proxyPassword": "proxy-secret", "arbitrarySecret": "must-not-export"
        }))
        .unwrap();
    account.status = "error".into();
    account.last_error = Some("must-not-export-error".into());
    store.upsert_account(&account).unwrap();

    let service = AuthorizationExportService::new(&store);
    let first = service
        .prepare(
            &owner.id,
            "2026-08-10T05:00:00.000Z",
            "portable-export-password",
            &codec,
            &PortableAuthorizationExportEncryptor,
        )
        .unwrap();
    let second = service
        .prepare(
            &owner.id,
            "2026-08-10T05:00:00.000Z",
            "portable-export-password",
            &codec,
            &PortableAuthorizationExportEncryptor,
        )
        .unwrap();
    assert_eq!(
        first.filename,
        "imail-mail-authorizations-2026-08-10.imailauth"
    );
    assert_eq!(first.account_count, 1);
    assert_eq!(first.envelope.format, MAIL_AUTHORIZATION_EXPORT_FORMAT);
    assert_eq!(
        first.envelope.format_version,
        MAIL_AUTHORIZATION_EXPORT_VERSION
    );
    assert_eq!(first.envelope.kdf.algorithm, "scrypt");
    assert_eq!(first.envelope.kdf.cost, 32_768);
    assert_eq!(first.envelope.cipher.algorithm, "aes-256-gcm");
    assert_ne!(first.envelope.kdf.salt, second.envelope.kdf.salt);
    assert_ne!(first.envelope.cipher.iv, second.envelope.cipher.iv);
    let serialized_envelope = serde_json::to_string(&first.envelope).unwrap();
    assert!(!serialized_envelope.contains("access-secret"));
    assert!(!serialized_envelope.contains("proxy-secret"));
    let encrypted = PortableEncryptedPayload {
        salt: first.envelope.kdf.salt.clone(),
        iv: first.envelope.cipher.iv.clone(),
        auth_tag: first.envelope.cipher.auth_tag.clone(),
        ciphertext: first.envelope.ciphertext.clone(),
    };
    let plaintext = decrypt_portable_export(
        &encrypted,
        "portable-export-password",
        b"imail-mail-authorizations:v1",
    )
    .unwrap();
    let payload: MailAuthorizationExportPayload = serde_json::from_slice(&plaintext).unwrap();
    assert_eq!(payload.accounts.len(), 1);
    assert_eq!(
        payload.accounts[0].authorization.access_token.as_deref(),
        Some("access-secret")
    );
    assert_eq!(
        payload.accounts[0].authorization.proxy_password.as_deref(),
        Some("proxy-secret")
    );
    let serialized_payload = serde_json::to_string(&payload).unwrap();
    for forbidden in [
        "arbitrarySecret",
        "must-not-export",
        "must-not-export-error",
        "encryptedSecret",
        "lastError",
        "mailboxes",
    ] {
        assert!(!serialized_payload.contains(forbidden));
    }
    assert!(decrypt_portable_export(
        &encrypted,
        "wrong-export-password",
        b"imail-mail-authorizations:v1"
    )
    .is_err());
    assert_eq!(
        service
            .prepare(
                &owner.id,
                "2026-08-10T05:00:00.000Z",
                "too-short",
                &codec,
                &PortableAuthorizationExportEncryptor,
            )
            .unwrap_err()
            .code(),
        "AUTHORIZATION_EXPORT_PASSWORD_INVALID"
    );
}

#[test]
fn rust_backup_round_trips_and_is_accepted_by_the_node_restore_preflight() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    fs::write(
        fixture.root.join("instance-id"),
        "11111111-1111-4111-8111-111111111111\n",
    )
    .unwrap();
    fs::create_dir(fixture.root.join("sender-logos")).unwrap();
    fs::write(
        fixture.root.join("sender-logos").join("logo.bin"),
        b"logo-data",
    )
    .unwrap();
    let active_before = fs::read(&database_path).unwrap();
    let backup_root = unique_root("imail-rust-backup");
    let rust_restore = unique_root("imail-rust-restore");
    let node_restore = unique_root("imail-node-restore");
    let tampered_restore = unique_root("imail-tampered-restore");

    let maintenance = DataMaintenanceService::new(&FilesystemDataMaintenance);
    let report = maintenance
        .create_backup(
            fixture.root.to_str().unwrap(),
            backup_root.to_str().unwrap(),
            "0.0.1",
            "2026-08-10T06:00:00.000Z",
        )
        .unwrap();
    assert_eq!(report.schema_version, 10);
    assert!(report.file_count >= 4);
    assert_eq!(fs::read(&database_path).unwrap(), active_before);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(backup_root.join("backup-manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["formatVersion"], 2);
    assert_eq!(manifest["service"], "imail");
    assert_eq!(manifest["serviceVersion"], "0.0.1");
    assert_eq!(manifest["schemaVersion"], 10);

    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute(
            "UPDATE accounts SET display_name='Changed after backup' WHERE id='a-first'",
            [],
        )
        .unwrap();
    drop(connection);
    let restored = maintenance
        .prepare_restore(
            backup_root.to_str().unwrap(),
            rust_restore.to_str().unwrap(),
        )
        .unwrap();
    assert!(restored.integrity_manifest_verified);
    assert!(restored.master_key_included);
    assert!(restored.instance_id_included);
    assert!(restored.sender_logos_included);
    let restored_connection = Connection::open(rust_restore.join("imail.sqlite")).unwrap();
    let restored_name: String = restored_connection
        .query_row(
            "SELECT display_name FROM accounts WHERE id='a-first'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(restored_name, "First");
    assert_eq!(
        fs::read_to_string(rust_restore.join("sender-logos").join("logo.bin")).unwrap(),
        "logo-data"
    );
    drop(restored_connection);

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let node = Command::new("node")
        .arg("scripts/prepare-restore.mjs")
        .arg(&backup_root)
        .arg(&node_restore)
        .current_dir(&workspace)
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "Node restore rejected Rust backup: {}",
        String::from_utf8_lossy(&node.stderr)
    );
    let node_report: serde_json::Value = serde_json::from_slice(&node.stdout).unwrap();
    assert_eq!(node_report["integrityManifestVerified"], true);
    assert_eq!(node_report["schemaVersion"], 10);

    fs::write(
        backup_root.join("sender-logos").join("logo.bin"),
        b"tampered",
    )
    .unwrap();
    assert!(prepare_data_restore(&backup_root, &tampered_restore).is_err());
    assert!(!tampered_restore.exists());
    assert!(prepare_data_restore(&backup_root, &rust_restore).is_err());

    for root in [backup_root, rust_restore, node_restore] {
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn rust_privacy_clear_is_user_scoped_and_preserves_identity_and_preferences() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id='__legacy__'", [])
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("clear-owner@example.com", "Owner", "Owner password 123!")
        .unwrap();
    let outsider = store
        .create_user("clear-other@example.com", "Other", "Other password 123!")
        .unwrap();
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute(
            "UPDATE accounts SET user_id=?1 WHERE id='a-later'",
            [&outsider.id],
        )
        .unwrap();
    drop(connection);
    store
        .set_user_metadata(&owner.id, "app_preferences_v1", r#"{"theme":"tech"}"#)
        .unwrap();
    for (user, profile_id) in [(&owner, "owner-deepl"), (&outsider, "other-deepl")] {
        TranslationSettingsService::new(&mut store)
            .upsert_profile(
                &user.id,
                profile_id,
                TranslationProviderProfileInput {
                    display_name: "DeepL".into(),
                    execution_target: TranslationExecutionTarget::LocalService,
                    provider: TranslationProviderConfiguration::DeepL {
                        plan: imail_protocol::DeepLApiPlan::Free,
                    },
                    enabled: true,
                },
                "2026-08-27T00:00:00.000Z",
            )
            .unwrap();
    }
    store
        .set_user_metadata(
            &owner.id,
            "translation_preferences_v1",
            r#"{"defaultTargetLanguage":"zh-Hans","autoTranslate":false,"cacheTranslations":true}"#,
        )
        .unwrap();
    let owner_token = store
        .issue_developer_token(
            &owner.id,
            "Owner token",
            &["messages:read".into()],
            &["a-first".into()],
            3_600,
        )
        .unwrap();
    let outsider_token = store
        .issue_developer_token(
            &outsider.id,
            "Other token",
            &["messages:read".into()],
            &["a-later".into()],
            3_600,
        )
        .unwrap();
    let owner_contact = ContactReadModel {
        owner_id: owner.id.clone(),
        address: "owner-contact@example.com".into(),
        name: "Owner contact".into(),
        message_count: 1,
        last_contact_at: "2026-08-10T00:00:00.000Z".into(),
        logo_key: None,
        logo_content_type: None,
        logo_source_url: None,
        logo_fetched_at: None,
    };
    let mut outsider_contact = owner_contact.clone();
    outsider_contact.owner_id.clone_from(&outsider.id);
    outsider_contact.address = "other-contact@example.com".into();
    store.upsert_contact(&owner_contact).unwrap();
    store.upsert_contact(&outsider_contact).unwrap();
    for (user_id, target) in [
        (&owner.id, "https://owner.example.com"),
        (&outsider.id, "https://other.example.com"),
    ] {
        store
            .upsert_logo_fetch_attempt(&LogoFetchAttemptRecord {
                owner_id: user_id.clone(),
                target: target.into(),
                domain_key: "domain:example.com".into(),
                status: "failed".into(),
                detail: "not found".into(),
                attempted_at: "2026-08-10T00:00:00.000Z".into(),
            })
            .unwrap();
    }
    store
        .upsert_draft(
            &owner.id,
            &DraftReadModel {
                id: "clear-draft".into(),
                account_id: "a-first".into(),
                to: json!([]),
                cc: json!([]),
                subject: "Clear".into(),
                text: String::new(),
                html: String::new(),
                attachments: json!([]),
                created_at: "2026-08-10T00:00:00.000Z".into(),
                updated_at: "2026-08-10T00:00:00.000Z".into(),
            },
        )
        .unwrap();

    let result = PrivacyService::new(&mut store)
        .clear_mail_data(&owner.id)
        .unwrap();
    assert_eq!(result.account_count, 1);
    assert!(store.list_accounts(&owner.id).unwrap().is_empty());
    assert!(store.list_messages(&owner.id).unwrap().is_empty());
    assert!(store.list_drafts(&owner.id).unwrap().is_empty());
    assert!(store.list_contacts(&owner.id).unwrap().is_empty());
    assert!(store
        .list_logo_fetch_attempts(&owner.id)
        .unwrap()
        .is_empty());
    assert!(store.list_developer_tokens(&owner.id).unwrap().is_empty());
    assert!(store
        .list_translation_providers(&owner.id)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .list_translation_providers(&outsider.id)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .user_metadata(&owner.id, "translation_preferences_v1")
        .unwrap()
        .is_none());
    assert!(store
        .authenticate_developer_token(&owner_token.raw, "messages:read")
        .unwrap()
        .is_none());
    assert_eq!(store.list_accounts(&outsider.id).unwrap().len(), 1);
    assert_eq!(store.list_contacts(&outsider.id).unwrap().len(), 1);
    assert_eq!(
        store.list_logo_fetch_attempts(&outsider.id).unwrap().len(),
        1
    );
    assert!(store
        .authenticate_developer_token(&outsider_token.raw, "messages:read")
        .unwrap()
        .is_some());
    assert_eq!(
        store
            .user_metadata(&owner.id, "app_preferences_v1")
            .unwrap()
            .as_deref(),
        Some(r#"{"theme":"tech"}"#)
    );
    assert!(store
        .authenticate("clear-owner@example.com", "Owner password 123!")
        .unwrap()
        .is_some());
}

#[test]
fn rust_privacy_clear_rolls_back_the_whole_operation_on_failure() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id='__legacy__'", [])
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("clear-rollback@example.com", "Owner", "Owner password 123!")
        .unwrap();
    let observer = Connection::open(&database_path).unwrap();
    observer
        .execute_batch(
            "CREATE TRIGGER reject_privacy_clear BEFORE DELETE ON accounts
             BEGIN SELECT RAISE(ABORT, 'intentional privacy rollback'); END;",
        )
        .unwrap();
    drop(observer);

    assert!(PrivacyService::new(&mut store)
        .clear_mail_data(&owner.id)
        .is_err());
    assert_eq!(store.list_accounts(&owner.id).unwrap().len(), 2);
    assert_eq!(store.list_messages(&owner.id).unwrap().len(), 1);
}

#[test]
fn rust_mail_overview_matches_notification_and_label_rules() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id='__legacy__'", [])
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("overview@example.com", "Overview", "Overview password 123!")
        .unwrap();
    let mut account = store.account(&owner.id, "a-first").unwrap().unwrap();
    account.status = "error".into();
    account.last_error = Some("Authentication failed".into());
    account.last_sync_at = Some("2026-08-10T03:00:00.000Z".into());
    store.upsert_account(&account).unwrap();
    let mut message = store.list_messages(&owner.id).unwrap().remove(0);
    message.snoozed_until = Some("2026-08-10T02:00:00.000Z".into());
    message.labels = json!(["客户", "important", "客户"]);
    store.upsert_message(&owner.id, &message).unwrap();

    let service = MailOverviewService::new(&store);
    assert_eq!(service.labels(&owner.id).unwrap(), ["客户", "important"]);
    let notifications = service
        .notifications(&owner.id, 30, "2026-08-10T04:00:00.000Z")
        .unwrap();
    assert!(notifications.iter().any(|item| {
        item.kind == NotificationKind::Error
            && item.id == "account-a-first"
            && item.detail == "Authentication failed"
    }));
    assert!(notifications
        .iter()
        .any(|item| { item.kind == NotificationKind::Snooze && item.id == "snooze-m1" }));
    assert!(notifications.iter().any(|item| {
        item.kind == NotificationKind::Unread && item.id == "unread-m1" && item.detail == "Sender"
    }));
    assert_eq!(
        service
            .notifications(&owner.id, 1, "2026-08-10T04:00:00.000Z")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn rust_custom_theme_accepts_only_safe_user_scoped_tokens() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("theme-owner@example.com", "Owner", "Owner password 123!")
        .unwrap();
    let outsider = store
        .create_user("theme-other@example.com", "Other", "Other password 123!")
        .unwrap();
    {
        let mut service = CustomThemeService::new(&mut store);
        assert_eq!(service.read(&owner.id).unwrap(), CustomTheme::default());
        let theme = CustomTheme {
            name: "  暮色珊瑚  ".into(),
            accent: "#D66F5F".into(),
            ..Default::default()
        };
        let updated = service.update(&owner.id, theme).unwrap();
        assert_eq!(updated.name, "暮色珊瑚");
        assert_eq!(service.read(&owner.id).unwrap(), updated);
        assert_eq!(service.read(&outsider.id).unwrap(), CustomTheme::default());
        let invalid = CustomTheme {
            canvas: "url(https://example.com)".into(),
            ..Default::default()
        };
        assert_eq!(
            service.update(&owner.id, invalid).unwrap_err().code(),
            "CUSTOM_THEME_INVALID"
        );
    }
    store
        .set_user_metadata(
            &owner.id,
            "mcp_custom_theme_v1",
            r##"{"name":"unsafe","canvas":"#ffffff","extra":"arbitrary-css"}"##,
        )
        .unwrap();
    assert_eq!(
        CustomThemeService::new(&mut store).read(&owner.id).unwrap(),
        CustomTheme::default()
    );
}

#[test]
fn rust_content_writes_enforce_direct_and_account_derived_ownership() {
    let fixture = Fixture::new(10);
    let database_path = fixture.root.join("imail.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute("UPDATE accounts SET user_id='__legacy__'", [])
        .unwrap();
    drop(connection);
    let mut store = SqliteAuthStore::open_database(&database_path).unwrap();
    let owner = store
        .create_user("content-owner@example.com", "Owner", "Owner password 123!")
        .unwrap();
    let outsider = store
        .create_user("content-other@example.com", "Other", "Other password 123!")
        .unwrap();
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute(
            "UPDATE accounts SET user_id=?1 WHERE id='a-later'",
            [&outsider.id],
        )
        .unwrap();
    drop(connection);

    let message = MessageReadModel {
        id: "content-message".into(),
        account_id: "a-first".into(),
        mailbox: "INBOX".into(),
        mailbox_role: "inbox".into(),
        uid: 88,
        message_id: Some("<content@example.com>".into()),
        from: json!({ "name": "Sender", "address": "sender@example.com" }),
        to: json!([{ "name": "Owner", "address": "first@example.com" }]),
        subject: "Content contract".into(),
        preview: "Preview".into(),
        text: "Body".into(),
        html: Some("<p>Body</p>".into()),
        date: "2026-08-10T01:00:00.000Z".into(),
        unread: true,
        flagged: false,
        has_attachments: true,
        attachments: json!([{ "filename": "a.txt", "contentType": "text/plain", "size": 1, "index": 0 }]),
        labels: json!(["important"]),
        snoozed_until: None,
    };
    store.upsert_message(&owner.id, &message).unwrap();
    assert!(store
        .list_messages(&owner.id)
        .unwrap()
        .iter()
        .any(|item| item == &message));
    let mut stolen_message = message.clone();
    stolen_message.account_id = "a-later".into();
    assert!(matches!(
        store.upsert_message(&outsider.id, &stolen_message),
        Err(AuthStoreError::ContentOwnershipViolation)
    ));
    assert!(!store.delete_message(&outsider.id, &message.id).unwrap());

    let mut draft = DraftReadModel {
        id: "content-draft".into(),
        account_id: "a-first".into(),
        to: json!(["recipient@example.com"]),
        cc: json!([]),
        subject: "Draft".into(),
        text: "Draft body".into(),
        html: "<p>Draft body</p>".into(),
        attachments: json!([]),
        created_at: "2026-08-10T02:00:00.000Z".into(),
        updated_at: "2026-08-10T02:00:00.000Z".into(),
    };
    store.upsert_draft(&owner.id, &draft).unwrap();
    draft.subject = "Updated draft".into();
    draft.created_at = "2099-01-01T00:00:00.000Z".into();
    store.upsert_draft(&owner.id, &draft).unwrap();
    let stored_draft = store
        .list_drafts(&owner.id)
        .unwrap()
        .into_iter()
        .find(|item| item.id == draft.id)
        .unwrap();
    assert_eq!(stored_draft.subject, "Updated draft");
    assert_eq!(stored_draft.created_at, "2026-08-10T02:00:00.000Z");
    let mut stolen_draft = draft.clone();
    stolen_draft.account_id = "a-later".into();
    assert!(matches!(
        store.upsert_draft(&outsider.id, &stolen_draft),
        Err(AuthStoreError::ContentOwnershipViolation)
    ));

    for user in [&owner, &outsider] {
        store
            .upsert_contact(&ContactReadModel {
                owner_id: user.id.clone(),
                address: "Person@Example.com".into(),
                name: user.display_name.clone(),
                message_count: 2,
                last_contact_at: "2026-08-10T03:00:00.000Z".into(),
                logo_key: Some("domain:example.com".into()),
                logo_content_type: Some("image/png".into()),
                logo_source_url: Some("https://example.com/logo.png".into()),
                logo_fetched_at: Some("2026-08-10T03:00:00.000Z".into()),
            })
            .unwrap();
        store
            .upsert_logo_fetch_attempt(&LogoFetchAttemptRecord {
                owner_id: user.id.clone(),
                target: "person@example.com".into(),
                domain_key: "example.com".into(),
                status: "success".into(),
                detail: "contract".into(),
                attempted_at: "2026-08-10T03:00:00.000Z".into(),
            })
            .unwrap();
    }
    assert_eq!(store.list_contacts(&owner.id).unwrap().len(), 1);
    assert_eq!(store.list_contacts(&outsider.id).unwrap().len(), 1);
    assert_eq!(store.list_logo_fetch_attempts(&owner.id).unwrap().len(), 1);
    assert!(store
        .delete_contact(&owner.id, "PERSON@example.com")
        .unwrap());
    assert_eq!(store.list_contacts(&owner.id).unwrap().len(), 0);
    assert_eq!(store.list_contacts(&outsider.id).unwrap().len(), 1);
    assert!(!store
        .delete_logo_fetch_attempt(&outsider.id, "missing@example.com")
        .unwrap());
    assert!(store
        .delete_logo_fetch_attempt(&owner.id, "person@example.com")
        .unwrap());
    assert_eq!(store.list_logo_fetch_attempts(&owner.id).unwrap().len(), 0);
    assert!(store.delete_draft(&owner.id, &draft.id).unwrap());
    assert!(store.delete_message(&owner.id, &message.id).unwrap());
}

fn raw_migration_fixture(sql: &str) -> Fixture {
    let root = unique_root("imail-rust-migration");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("master.key"), "42".repeat(32)).unwrap();
    let connection = Connection::open(root.join("imail.sqlite")).unwrap();
    connection.execute_batch(sql).unwrap();
    drop(connection);
    Fixture { root }
}

#[test]
fn rust_migrates_v8_to_v10_and_encrypts_translation_provider_credentials() {
    let fixture = raw_migration_fixture(include_str!("../sql/schema-v8.sql"));
    let connection = Connection::open(fixture.root.join("imail.sqlite")).unwrap();
    connection
        .execute(
            "INSERT INTO metadata (key,value) VALUES ('schema_version','8')",
            [],
        )
        .unwrap();
    drop(connection);

    let report = migrate_database(fixture.root.join("imail.sqlite")).unwrap();
    assert_eq!(report.from_version, 8);
    assert_eq!(report.to_version, 10);
    assert_eq!(report.applied_versions, [9, 10]);

    let mut store = SqliteAuthStore::open_database(fixture.root.join("imail.sqlite")).unwrap();
    let user = store
        .create_user("translator", "Translator", "translation-password-123")
        .unwrap();
    let master_key = MasterKey::from_file(fixture.root.join("master.key")).unwrap();
    let codec = MasterKeyCredentialCodec::new(&master_key);
    let profile_id = "deepl-local-1";
    TranslationSettingsService::new(&mut store)
        .upsert_profile(
            &user.id,
            profile_id,
            TranslationProviderProfileInput {
                display_name: "DeepL Free".into(),
                execution_target: TranslationExecutionTarget::LocalService,
                provider: TranslationProviderConfiguration::DeepL {
                    plan: imail_protocol::DeepLApiPlan::Free,
                },
                enabled: true,
            },
            "2026-08-27T00:00:00.000Z",
        )
        .unwrap();
    assert!(TranslationSettingsService::new(&mut store)
        .update(
            &user.id,
            imail_protocol::TranslationSettingsUpdate {
                preferences: imail_protocol::TranslationPreferences::default(),
                environment: imail_protocol::TranslationEnvironmentPreferences {
                    default_profile_id: Some(profile_id.into()),
                },
            },
        )
        .is_err());
    TranslationSettingsService::new(&mut store)
        .set_credential(
            &user.id,
            profile_id,
            TranslationCredentialSecret {
                kind: TranslationCredentialKind::DeepLApiKey,
                secret: "deepl-secret-value".into(),
            },
            &codec,
            "2026-08-27T00:01:00.000Z",
        )
        .unwrap();

    let record = store
        .translation_provider(&user.id, profile_id)
        .unwrap()
        .unwrap();
    let encrypted = record.encrypted_credential.unwrap();
    assert!(!encrypted.contains("deepl-secret-value"));
    assert_eq!(
        master_key
            .decrypt_json::<serde_json::Value>(&encrypted)
            .unwrap(),
        json!({"kind":"deepl-api-key", "secret":"deepl-secret-value"})
    );
    let settings = TranslationSettingsService::new(&mut store)
        .read(&user.id)
        .unwrap();
    let serialized = serde_json::to_string(&settings).unwrap();
    assert!(!serialized.contains("deepl-secret-value"));
    assert_eq!(
        settings.profiles[0].status,
        imail_protocol::TranslationProviderStatus::NeedsConsent
    );
    assert!(settings.environment.default_profile_id.is_none());
    let settings = TranslationSettingsService::new(&mut store)
        .accept_consent(&user.id, profile_id, "2026-08-27T00:02:00.000Z")
        .unwrap();
    assert_eq!(
        settings.profiles[0].status,
        imail_protocol::TranslationProviderStatus::Configured
    );
    let settings = TranslationSettingsService::new(&mut store)
        .update(
            &user.id,
            imail_protocol::TranslationSettingsUpdate {
                preferences: imail_protocol::TranslationPreferences::default(),
                environment: imail_protocol::TranslationEnvironmentPreferences {
                    default_profile_id: Some(profile_id.into()),
                },
            },
        )
        .unwrap();
    assert_eq!(
        settings.environment.default_profile_id.as_deref(),
        Some(profile_id)
    );
    let settings = TranslationSettingsService::new(&mut store)
        .revoke_consent(&user.id, profile_id, "2026-08-27T00:03:00.000Z")
        .unwrap();
    assert_eq!(
        settings.profiles[0].status,
        imail_protocol::TranslationProviderStatus::NeedsConsent
    );
    assert!(settings.environment.default_profile_id.is_none());
    let settings = TranslationSettingsService::new(&mut store)
        .clear_credential(&user.id, profile_id, "2026-08-27T00:04:00.000Z")
        .unwrap();
    assert_eq!(
        settings.profiles[0].status,
        imail_protocol::TranslationProviderStatus::NeedsCredential
    );
    let cleared = store
        .translation_provider(&user.id, profile_id)
        .unwrap()
        .unwrap();
    assert!(cleared.encrypted_credential.is_none());
    assert!(cleared.profile.credential.is_none());
}

#[test]
fn rust_migrates_v7_to_v10_with_local_hme_cache_tables() {
    let fixture = raw_migration_fixture(
        "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
         INSERT INTO metadata VALUES ('schema_version','7');
         CREATE TABLE accounts (id TEXT PRIMARY KEY, user_id TEXT NOT NULL) STRICT;
         INSERT INTO accounts VALUES ('icloud-1','user-1');",
    );
    let report = migrate_database(fixture.root.join("imail.sqlite")).unwrap();
    assert_eq!(report.from_version, 7);
    assert_eq!(report.to_version, 10);
    assert_eq!(report.applied_versions, [8, 9, 10]);
    let connection = Connection::open(fixture.root.join("imail.sqlite")).unwrap();
    for table in ["apple_hme_addresses", "apple_hme_sync_state"] {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "missing {table}");
    }
}

#[test]
fn rust_migrates_v2_sync_schema_to_v10_without_losing_valid_rows() {
    let fixture = raw_migration_fixture(
        r#"
        PRAGMA foreign_keys=ON;
        CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
        INSERT INTO metadata VALUES ('schema_version','2');
        INSERT INTO metadata VALUES ('sync_default_policy:user-1','{"enabled":true,"intervalMinutes":5,"folderMode":"standard","selectedMailboxes":[],"syncOnStart":true,"retryOnRecovery":true,"notifyOnError":false}');
        CREATE TABLE accounts (
          id TEXT PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL COLLATE NOCASE,
          display_name TEXT NOT NULL, group_name TEXT NOT NULL, group_icon TEXT NOT NULL DEFAULT 'folder',
          color TEXT NOT NULL, settings_json TEXT NOT NULL, encrypted_secret TEXT NOT NULL,
          auth_method TEXT, created_at TEXT NOT NULL, last_sync_at TEXT, status TEXT NOT NULL,
          last_error TEXT, mailboxes_json TEXT NOT NULL DEFAULT '[]', user_id TEXT NOT NULL DEFAULT '__legacy__',
          UNIQUE(user_id,email)
        ) STRICT;
        INSERT INTO accounts VALUES ('account-1','gmail','owner@example.com','Owner','Personal','folder','#168f78','{}','cipher','oauth2','2026-07-30T00:00:00.000Z',NULL,'connected',NULL,'[]','user-1');
        CREATE TABLE sync_policies (account_id TEXT PRIMARY KEY, enabled INTEGER NOT NULL, interval_minutes INTEGER NOT NULL, folder_mode TEXT NOT NULL, selected_mailboxes_json TEXT NOT NULL, sync_on_start INTEGER NOT NULL, retry_on_recovery INTEGER NOT NULL, notify_on_error INTEGER NOT NULL, updated_at TEXT NOT NULL) STRICT;
        CREATE TABLE mailbox_sync_states (account_id TEXT NOT NULL, mailbox TEXT NOT NULL, mailbox_role TEXT NOT NULL, uid_validity TEXT, last_seen_uid INTEGER NOT NULL, highest_modseq TEXT, last_attempt_at TEXT, last_success_at TEXT, next_sync_at TEXT, consecutive_failures INTEGER NOT NULL, connection_status TEXT NOT NULL, sync_state TEXT NOT NULL, last_error_code TEXT, last_error_message TEXT, PRIMARY KEY(account_id,mailbox)) STRICT;
        CREATE TABLE sync_jobs (id TEXT PRIMARY KEY, account_id TEXT NOT NULL, mailbox TEXT, mailbox_role TEXT NOT NULL, reason TEXT NOT NULL, status TEXT NOT NULL, priority INTEGER NOT NULL, not_before TEXT NOT NULL, locked_by TEXT, locked_until TEXT, attempts INTEGER NOT NULL, created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, synced_count INTEGER, new_count INTEGER, updated_count INTEGER, deleted_count INTEGER, error_code TEXT, error_message TEXT) STRICT;
        CREATE TABLE sync_events (id INTEGER PRIMARY KEY AUTOINCREMENT, event_type TEXT NOT NULL, account_id TEXT NOT NULL, job_id TEXT, payload_json TEXT NOT NULL, created_at TEXT NOT NULL) STRICT;
        INSERT INTO sync_policies VALUES ('account-1',1,5,'inbox','[]',1,1,1,'2026-07-30T00:00:00.000Z');
        INSERT INTO sync_jobs VALUES ('job-1','account-1',NULL,'inbox','manual','succeeded',0,'2026-07-30T00:00:00.000Z',NULL,NULL,1,'2026-07-30T00:00:00.000Z',NULL,NULL,0,0,0,0,NULL,NULL);
        INSERT INTO sync_events(event_type,account_id,job_id,payload_json,created_at) VALUES ('sync.completed','account-1','job-1','{}','2026-07-30T00:00:00.000Z');
        "#,
    );
    let report = migrate_database(fixture.root.join("imail.sqlite")).unwrap();
    assert_eq!(report.from_version, 2);
    assert_eq!(report.to_version, 10);
    assert_eq!(report.applied_versions, [3, 4, 5, 6, 7, 8, 9, 10]);
    let connection = Connection::open(fixture.root.join("imail.sqlite")).unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    let version: String = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "10");
    let policy_columns = connection
        .prepare("SELECT name FROM pragma_table_info('sync_policies') ORDER BY cid")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!policy_columns.contains(&"interval_minutes".into()));
    let rerun: i64 = connection
        .query_row(
            "SELECT rerun_requested FROM sync_jobs WHERE id='job-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rerun, 0);
    let proxy: Option<String> = connection
        .query_row(
            "SELECT proxy_json FROM accounts WHERE id='account-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(proxy.is_none());
    let policy: serde_json::Value = serde_json::from_str(
        &connection
            .query_row(
                "SELECT value FROM metadata WHERE key='sync_default_policy:user-1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        policy,
        json!({ "enabled": true, "folderMode": "standard", "selectedMailboxes": [], "notifyOnError": false })
    );
    assert_eq!(
        connection
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .mapped(|_| Ok(()))
            .count(),
        0
    );
    connection
        .execute("DELETE FROM accounts WHERE id='account-1'", [])
        .unwrap();
    let remaining: i64 = connection
        .query_row(
            "SELECT (SELECT count(*) FROM sync_policies)+(SELECT count(*) FROM sync_jobs)+(SELECT count(*) FROM sync_events)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
}

#[test]
fn rust_migrates_unversioned_legacy_columns_and_owner_data_to_v6() {
    let fixture = raw_migration_fixture(
        r#"
        CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
        CREATE TABLE accounts (
          id TEXT PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL COLLATE NOCASE UNIQUE,
          display_name TEXT NOT NULL, group_name TEXT NOT NULL, color TEXT NOT NULL,
          settings_json TEXT NOT NULL, encrypted_secret TEXT NOT NULL, auth_method TEXT,
          created_at TEXT NOT NULL, last_sync_at TEXT, status TEXT NOT NULL, last_error TEXT
        ) STRICT;
        INSERT INTO accounts VALUES ('legacy-account','gmail','legacy@example.com','Legacy','Personal','#168f78','{}','cipher','oauth2','2026-01-01T00:00:00.000Z',NULL,'connected',NULL);
        CREATE TABLE messages (
          id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
          mailbox TEXT NOT NULL, uid INTEGER NOT NULL, message_id TEXT, from_json TEXT NOT NULL,
          to_json TEXT NOT NULL, subject TEXT NOT NULL, preview TEXT NOT NULL, text_body TEXT NOT NULL,
          html_body TEXT, received_at TEXT NOT NULL, unread INTEGER NOT NULL, flagged INTEGER NOT NULL,
          has_attachments INTEGER NOT NULL, attachments_json TEXT NOT NULL
        ) STRICT;
        INSERT INTO messages VALUES ('legacy-message','legacy-account','INBOX',1,NULL,'{"name":"Sender","address":"sender@example.com"}','[]','Legacy','Preview','Body',NULL,'2026-01-01T01:00:00.000Z',1,0,0,'[]');
        CREATE TABLE contacts (address TEXT PRIMARY KEY COLLATE NOCASE, name TEXT NOT NULL, message_count INTEGER NOT NULL, last_contact_at TEXT NOT NULL, logo_key TEXT, logo_content_type TEXT, logo_source_url TEXT, logo_fetched_at TEXT) STRICT;
        INSERT INTO contacts VALUES ('person@example.com','Person',1,'2026-01-01T01:00:00.000Z',NULL,NULL,NULL,NULL);
        CREATE TABLE logo_fetch_attempts (target TEXT PRIMARY KEY, domain_key TEXT NOT NULL, status TEXT NOT NULL, detail TEXT NOT NULL, attempted_at TEXT NOT NULL) STRICT;
        INSERT INTO logo_fetch_attempts VALUES ('person@example.com','example.com','failed','legacy','2026-01-01T01:00:00.000Z');
        CREATE TABLE drafts (id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, to_json TEXT NOT NULL, cc_json TEXT NOT NULL, subject TEXT NOT NULL, text_body TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL) STRICT;
        INSERT INTO drafts VALUES ('legacy-draft','legacy-account','[]','[]','Draft','Body','2026-01-01T00:00:00.000Z','2026-01-01T00:00:00.000Z');
        CREATE TABLE developer_tokens (id TEXT PRIMARY KEY, name TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE, prefix TEXT NOT NULL, created_at TEXT NOT NULL, expires_at TEXT NOT NULL, last_used_at TEXT) STRICT;
        INSERT INTO developer_tokens VALUES ('legacy-token','Legacy','hash','imail_legacy','2026-01-01T00:00:00.000Z','2026-01-02T00:00:00.000Z',NULL);
        "#,
    );
    let report = migrate_database(fixture.root.join("imail.sqlite")).unwrap();
    assert_eq!(report.from_version, 0);
    assert_eq!(report.applied_versions, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    let connection = Connection::open(fixture.root.join("imail.sqlite")).unwrap();
    let account: (String, String, String, Option<String>) = connection
        .query_row(
            "SELECT user_id,group_icon,mailboxes_json,proxy_json FROM accounts WHERE id='legacy-account'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        account,
        ("__legacy__".into(), "folder".into(), "[]".into(), None)
    );
    let message: (String, String, Option<String>) = connection
        .query_row(
            "SELECT mailbox_role,labels_json,snoozed_until FROM messages WHERE id='legacy-message'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(message, ("inbox".into(), "[]".into(), None));
    let pagination_index: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='messages_account_role_unread_date_id'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(pagination_index, 1);
    let draft: (String, String) = connection
        .query_row(
            "SELECT html_body,attachments_json FROM drafts WHERE id='legacy-draft'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(draft, (String::new(), "[]".into()));
    for table in ["contacts", "logo_fetch_attempts", "developer_tokens"] {
        let owner: String = connection
            .query_row(&format!("SELECT user_id FROM {table} LIMIT 1"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(owner, "__legacy__");
    }
}

#[test]
fn rust_migration_rolls_back_all_steps_on_invalid_legacy_policy() {
    let fixture = raw_migration_fixture(
        r#"
        CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
        INSERT INTO metadata VALUES ('schema_version','5');
        CREATE TABLE accounts (id TEXT PRIMARY KEY, email TEXT NOT NULL, proxy_json TEXT) STRICT;
        CREATE TABLE sync_policies (account_id TEXT PRIMARY KEY, enabled INTEGER NOT NULL, interval_minutes INTEGER NOT NULL, folder_mode TEXT NOT NULL, selected_mailboxes_json TEXT NOT NULL, sync_on_start INTEGER NOT NULL, retry_on_recovery INTEGER NOT NULL, notify_on_error INTEGER NOT NULL, updated_at TEXT NOT NULL) STRICT;
        INSERT INTO accounts VALUES ('account-1','owner@example.com',NULL);
        INSERT INTO sync_policies VALUES ('account-1',1,5,'invalid','[]',1,1,1,'2026-08-10T00:00:00.000Z');
        "#,
    );
    assert!(matches!(
        migrate_database(fixture.root.join("imail.sqlite")),
        Err(MigrationError::Sqlite(_))
    ));
    let connection = Connection::open(fixture.root.join("imail.sqlite")).unwrap();
    let version: String = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "5");
    let still_legacy: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('sync_policies') WHERE name='interval_minutes')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(still_legacy);
    let temporary_tables: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE '%_push_first'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(temporary_tables, 0);
}

#[test]
fn rust_migration_rejects_future_schema_without_committing_schema_changes() {
    let fixture = raw_migration_fixture(
        "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
         INSERT INTO metadata VALUES ('schema_version','999');",
    );
    let before = fs::read(fixture.root.join("imail.sqlite")).unwrap();
    assert!(matches!(
        migrate_database(fixture.root.join("imail.sqlite")),
        Err(MigrationError::FutureSchema { actual: 999, .. })
    ));
    assert_eq!(fs::read(fixture.root.join("imail.sqlite")).unwrap(), before);
}

#[test]
fn rust_migration_serializes_concurrent_openers_and_rechecks_version() {
    let fixture = raw_migration_fixture(
        "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
         INSERT INTO metadata VALUES ('schema_version','4');
         CREATE TABLE accounts (id TEXT PRIMARY KEY, email TEXT NOT NULL) STRICT;
         INSERT INTO accounts VALUES ('account-1','owner@example.com');",
    );
    let left_path = fixture.root.join("imail.sqlite");
    let right_path = left_path.clone();
    let left = std::thread::spawn(move || migrate_database(left_path).unwrap());
    let right = std::thread::spawn(move || migrate_database(right_path).unwrap());
    let reports = [left.join().unwrap(), right.join().unwrap()];
    assert!(reports.iter().any(|report| report.from_version == 4));
    assert!(reports.iter().any(|report| report.from_version == 10));
    assert!(reports.iter().all(|report| report.to_version == 10));
}
