//! HTTP contract tests with shared local fixtures.

mod account_contracts;
mod auth_contracts;
mod boundary_contracts;
mod draft_contracts;
mod external_access_contracts;
mod gateway_contracts;
mod host_contracts;
mod logo_contracts;
mod mcp_contracts;
mod message_contracts;
mod oauth_contracts;
mod settings_contracts;
mod sync_contracts;
mod work_queue_contracts;

mod real_mail_fixture;
#[path = "../rules_http_tests.rs"]
mod rules_http_tests;
#[path = "../search_http_tests.rs"]
mod search_http_tests;

use crate::host::start_sync_runtime;
use crate::*;
use axum::{
    body::to_bytes,
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        Request,
    },
};
use axum::{
    body::Body,
    http::{
        header::{
            ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, HOST,
            ORIGIN,
        },
        HeaderValue, Method, StatusCode,
    },
    response::Response,
};
use futures_util::StreamExt;
use imail_core::{
    AccountRecord, AccountRepository, AuthRepository, ContentRepository, DeveloperTokenRepository,
};
use imail_mail::{ImapPort, SmtpPort};
use imail_mail::{
    MailAuthentication, MailConnectionConfig, OutgoingMessage, ProtocolFailure, ProtocolStage,
    RemoteMailbox, RemoteMessageLocator, RemoteMoveConfirmation,
};
use imail_oauth::{OAuthConfig, OAuthError, OAuthGrant, OAuthIdentity, OAuthTokenResponse};
use imail_oauth::{OAuthEnvironment, OAuthProviderPort, OAuthProviderPortFactory};

use imail_security::{decrypt_portable_export, MasterKey, PortableEncryptedPayload};
use imail_storage_sqlite::{
    migrate_database, AppleHmeAddressRecord, SqliteAuthStore, SyncEnqueue, SyncRuntimeStore,
};
use serde_json::{json, Value};
use std::{
    fs,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Default)]
struct MailTestState {
    flag_updates: usize,
    moves: usize,
    fetches: usize,
    sends: usize,
    last_sent: Option<OutgoingMessage>,
    reject_flags: bool,
}

struct TestMailTransportFactory {
    state: Arc<Mutex<MailTestState>>,
    source: Vec<u8>,
}

struct TestLogoDiscovery {
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

impl logo::LogoDiscoveryPort for TestLogoDiscovery {
    fn discover(
        &self,
        source: &logo::LogoSource,
        _: &[imail_core::LogoFetchAttemptRecord],
    ) -> logo::LogoDiscoveryReport {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(source.address, "sender@example.org");
        logo::LogoDiscoveryReport {
            result: Some(logo::DiscoveredLogo {
                content: vec![137, 80, 78, 71, 13, 10, 26, 10, 1],
                content_type: "image/png".into(),
                source_url: "https://example.org/favicon.png".into(),
                fetched_at: "2026-08-10T09:00:00.000Z".into(),
                key: "domain:example.org".into(),
            }),
            permanent_failure: false,
            attempts: vec![logo::LogoAttempt {
                target: "https://example.org".into(),
                domain_key: "domain:example.org".into(),
                status: "success".into(),
                detail: "https://example.org/favicon.png".into(),
                attempted_at: "2026-08-10T09:00:00.000Z".into(),
            }],
        }
    }
}

impl MailTransportFactory for TestMailTransportFactory {
    fn create_imap(&self) -> Result<Box<dyn ImapPort>, String> {
        Ok(Box::new(TestImap {
            state: Arc::clone(&self.state),
            source: self.source.clone(),
        }))
    }

    fn create_smtp(&self) -> Result<Box<dyn SmtpPort>, String> {
        Ok(Box::new(TestSmtp {
            state: Arc::clone(&self.state),
        }))
    }
}

struct TestImap {
    state: Arc<Mutex<MailTestState>>,
    source: Vec<u8>,
}

impl ImapPort for TestImap {
    fn verify(&mut self, _: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
        Ok(())
    }

    fn fetch_source(
        &mut self,
        _: &MailConnectionConfig,
        _: &RemoteMessageLocator,
    ) -> Result<Vec<u8>, ProtocolFailure> {
        self.state.lock().unwrap().fetches += 1;
        Ok(self.source.clone())
    }

    fn update_flags(
        &mut self,
        _: &MailConnectionConfig,
        _: &RemoteMessageLocator,
        _: &imail_protocol::RemoteMessageFlagPatch,
    ) -> Result<(), ProtocolFailure> {
        let mut state = self.state.lock().unwrap();
        state.flag_updates += 1;
        if state.reject_flags {
            return Err(ProtocolFailure::from_provider(
                ProtocolStage::Imap,
                None,
                "authorization=remote-secret",
            ));
        }
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
        _: &RemoteMessageLocator,
        target: &str,
    ) -> Result<RemoteMoveConfirmation, ProtocolFailure> {
        assert_eq!(target, "Archive");
        self.state.lock().unwrap().moves += 1;
        Ok(RemoteMoveConfirmation {
            confirmed: true,
            uid: Some(44),
        })
    }
}

struct TestSmtp {
    state: Arc<Mutex<MailTestState>>,
}

impl SmtpPort for TestSmtp {
    fn verify(&mut self, _: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
        Ok(())
    }

    fn send(
        &mut self,
        _: &MailConnectionConfig,
        message: &OutgoingMessage,
    ) -> Result<imail_protocol::SendMessageResult, ProtocolFailure> {
        let mut state = self.state.lock().unwrap();
        state.sends += 1;
        state.last_sent = Some(message.clone());
        Ok(imail_protocol::SendMessageResult {
            message_id: "<sent@example.com>".into(),
            accepted: message.to.clone(),
        })
    }
}

fn temporary_directory(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("imail-http-{label}-{}", Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn authentication_directory(label: &str) -> PathBuf {
    let directory = temporary_directory(label);
    fs::File::create(directory.join("imail.sqlite")).unwrap();
    migrate_database(directory.join("imail.sqlite")).unwrap();
    directory
}

async fn json(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn text_body(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}
