use crate::request_cancellation::{wait_for_request_cancellation, RequestCancellationRegistry};
use axum::http::HeaderValue;
#[cfg(test)]
use axum::{
    body::{to_bytes, Body},
    http::{
        header::{COOKIE, HOST, SET_COOKIE},
        HeaderMap, Method, Request,
    },
};
use chrono::{SecondsFormat, Utc};
use imail_core::{
    accounts::AccountService,
    authentication::{AuthenticationError, AuthenticationService, LoginInput, RegistrationInput},
    developer_tokens::{CreateDeveloperTokenInput, DeveloperTokenService},
    drafts::DraftService,
    external_access::{ExternalAccessChanges, ExternalAccessService},
    messages::MessageQueryService,
    notifications::MailOverviewService,
    preferences::PreferencesService,
    ApplicationError, AuthRepository, ContentRepository,
};
use imail_http::{EmbeddedServiceHost, HttpAdapterConfig};
use imail_oauth::OAuthEnvironment;
use imail_protocol::{
    AccountMetadataPatch, AppPreferencesPatch, TranslationProviderProfileInput,
    TranslationSettingsUpdate,
};
use imail_security::MasterKey;
use imail_storage_sqlite::{migrate_database, SqliteAuthStore, SyncRuntimeStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    fs::OpenOptions,
    io::Write,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::OnceCell;
#[cfg(test)]
use tower::ServiceExt;

const MAX_REQUEST_BYTES: usize = 25 * 1024 * 1024;
const EXTERNAL_HTTP_PORT_FILE: &str = "external-http-port";

fn desktop_oauth_environment() -> OAuthEnvironment {
    OAuthEnvironment {
        callback_base_url: "http://127.0.0.1:0/api/oauth".into(),
        google_client_id: option_env!("GOOGLE_OAUTH_DESKTOP_CLIENT_ID").map(str::to_owned),
        google_client_secret: option_env!("GOOGLE_OAUTH_DESKTOP_CLIENT_SECRET").map(str::to_owned),
        microsoft_client_id: option_env!("MICROSOFT_OAUTH_DESKTOP_CLIENT_ID").map(str::to_owned),
        ..OAuthEnvironment::default()
    }
}

#[derive(Debug)]
struct EmbeddedServiceRequest {
    path: String,
    #[allow(dead_code)]
    method: String,
    body: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EmbeddedServiceResponse {
    status: u16,
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase")]
pub enum EmbeddedDomainCall {
    SystemInfo,
    Providers,
    AuthStatus,
    AuthRegister {
        input: serde_json::Value,
    },
    AuthLogin {
        input: serde_json::Value,
    },
    AuthLogout,
    AccountsList,
    AccountCreate {
        input: serde_json::Value,
    },
    AccountUpdate {
        #[serde(rename = "accountId")]
        account_id: String,
        input: serde_json::Value,
    },
    AccountDelete {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    AccountCredentialUpdate {
        #[serde(rename = "accountId")]
        account_id: String,
        input: serde_json::Value,
    },
    AccountProxyUpdate {
        #[serde(rename = "accountId")]
        account_id: String,
        input: serde_json::Value,
    },
    AccountConnectionTest {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    AppleHmeStatus {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    AppleHmeStartLogin {
        #[serde(rename = "accountId")]
        account_id: String,
        input: serde_json::Value,
    },
    AppleHmeSubmitTwoFactor {
        #[serde(rename = "accountId")]
        account_id: String,
        input: serde_json::Value,
    },
    AppleHmeList {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    AppleHmeSync {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    AppleHmeCreate {
        #[serde(rename = "accountId")]
        account_id: String,
        input: serde_json::Value,
    },
    AppleHmeDeactivate {
        #[serde(rename = "accountId")]
        account_id: String,
        #[serde(rename = "anonymousId")]
        anonymous_id: String,
    },
    AppleHmeDelete {
        #[serde(rename = "accountId")]
        account_id: String,
        #[serde(rename = "anonymousId")]
        anonymous_id: String,
    },
    AppleHmeDisconnect {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    OauthStart {
        input: serde_json::Value,
    },
    OauthReconnect {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    OauthStatus {
        input: serde_json::Value,
    },
    MessageStats,
    MessagesList {
        query: BTreeMap<String, String>,
    },
    MessageDetail {
        #[serde(rename = "messageId")]
        message_id: String,
    },
    MessageSource {
        #[serde(rename = "messageId")]
        message_id: String,
    },
    MessageConversation {
        #[serde(rename = "messageId")]
        message_id: String,
    },
    SyncAll,
    SyncAccount {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    SyncAccountMailbox {
        #[serde(rename = "accountId")]
        account_id: String,
        mailbox: String,
    },
    SyncMailboxRole {
        role: String,
    },
    MessageUpdate {
        #[serde(rename = "messageId")]
        message_id: String,
        unread: Option<bool>,
        flagged: Option<bool>,
        labels: Option<Vec<String>>,
        #[serde(rename = "snoozedUntil")]
        snoozed_until: Option<serde_json::Value>,
    },
    MessageMove {
        #[serde(rename = "messageId")]
        message_id: String,
        destination: String,
    },
    AttachmentPreviewCreate {
        #[serde(rename = "messageId")]
        message_id: String,
        index: usize,
    },
    AttachmentPreviewDelete {
        #[serde(rename = "previewId")]
        preview_id: String,
    },
    LabelsList,
    ContactsList,
    NotificationsList,
    MessageSend {
        input: serde_json::Value,
    },
    OutboxList,
    OutboxSchedule {
        input: serde_json::Value,
    },
    OutboxCancel {
        #[serde(rename = "itemId")]
        item_id: String,
    },
    OutboxRetry {
        #[serde(rename = "itemId")]
        item_id: String,
    },
    OutboxResolve {
        #[serde(rename = "itemId")]
        item_id: String,
        input: serde_json::Value,
    },
    MailWorkItemsList,
    MailWorkItemSet {
        #[serde(rename = "messageId")]
        message_id: String,
        input: serde_json::Value,
    },
    MailWorkItemComplete {
        #[serde(rename = "messageId")]
        message_id: String,
    },
    MailReplyDraftCreate {
        #[serde(rename = "messageId")]
        message_id: String,
        input: serde_json::Value,
    },
    MailDraftSchedule {
        #[serde(rename = "draftId")]
        draft_id: String,
        input: serde_json::Value,
    },
    DraftsList,
    DraftCreate {
        #[serde(rename = "draftId")]
        draft_id: Option<String>,
        input: serde_json::Value,
    },
    DraftUpdate {
        #[serde(rename = "draftId")]
        draft_id: String,
        input: serde_json::Value,
    },
    DraftDelete {
        #[serde(rename = "draftId")]
        draft_id: String,
    },
    PreferencesGet,
    SmartFoldersList,
    MailRulesList,
    MailRuleGet {
        #[serde(rename = "ruleId")]
        rule_id: String,
    },
    MailRuleCreate {
        input: serde_json::Value,
    },
    MailRuleUpdate {
        #[serde(rename = "ruleId")]
        rule_id: String,
        input: serde_json::Value,
    },
    MailRuleSetEnabled {
        #[serde(rename = "ruleId")]
        rule_id: String,
        input: serde_json::Value,
    },
    MailRuleDelete {
        #[serde(rename = "ruleId")]
        rule_id: String,
    },
    MailRulePreview {
        input: serde_json::Value,
    },
    MailRuleApply {
        input: serde_json::Value,
    },
    MailRuleRuns,
    MailRuleRetry {
        #[serde(rename = "runId")]
        run_id: String,
    },
    SmartFolderCreate {
        input: serde_json::Value,
    },
    SmartFolderUpdate {
        #[serde(rename = "folderId")]
        folder_id: String,
        input: serde_json::Value,
    },
    SmartFolderDelete {
        #[serde(rename = "folderId")]
        folder_id: String,
    },
    PreferencesUpdate {
        input: serde_json::Value,
    },
    TranslationSettingsGet,
    TranslationSettingsUpdate {
        input: serde_json::Value,
    },
    TranslationProfileUpsert {
        #[serde(rename = "profileId")]
        profile_id: String,
        input: serde_json::Value,
    },
    TranslationProfileDelete {
        #[serde(rename = "profileId")]
        profile_id: String,
    },
    TranslationCredentialUpdate {
        #[serde(rename = "profileId")]
        profile_id: String,
        input: serde_json::Value,
    },
    TranslationCredentialClear {
        #[serde(rename = "profileId")]
        profile_id: String,
    },
    TranslationConsentAccept {
        #[serde(rename = "profileId")]
        profile_id: String,
    },
    TranslationConsentRevoke {
        #[serde(rename = "profileId")]
        profile_id: String,
    },
    TranslationPrepare {
        #[serde(rename = "messageId")]
        message_id: String,
        input: serde_json::Value,
    },
    TranslationExecute {
        #[serde(rename = "messageId")]
        message_id: String,
        input: serde_json::Value,
    },
    TranslationComplete {
        #[serde(rename = "messageId")]
        message_id: String,
        input: serde_json::Value,
    },
    TranslationCacheClear,
    DeveloperTokensList,
    DeveloperTokenCreate {
        input: serde_json::Value,
    },
    DeveloperTokenDelete {
        #[serde(rename = "tokenId")]
        token_id: String,
    },
    ExternalAccessGet,
    ExternalAccessUpdate {
        input: serde_json::Value,
    },
    AuthorizationExportPrepare {
        input: serde_json::Value,
    },
    UserDataClear {
        input: serde_json::Value,
    },
}

#[cfg(test)]
struct EmbeddedInvocation {
    request: EmbeddedServiceRequest,
    draft_id: Option<String>,
}

#[cfg(test)]
impl EmbeddedDomainCall {
    fn into_invocation(self) -> EmbeddedInvocation {
        let mut draft_id = None;
        let (path, method, body) = match self {
            Self::SystemInfo => ("/api/system/info".into(), "GET", None),
            Self::Providers => ("/api/providers".into(), "GET", None),
            Self::AuthStatus => ("/api/auth/status".into(), "GET", None),
            Self::AuthRegister { input } => json_request("/api/auth/register", "POST", input),
            Self::AuthLogin { input } => json_request("/api/auth/login", "POST", input),
            Self::AuthLogout => ("/api/auth/logout".into(), "POST", None),
            Self::AccountsList => ("/api/accounts".into(), "GET", None),
            Self::AccountCreate { input } => json_request("/api/accounts", "POST", input),
            Self::AccountUpdate { account_id, input } => json_request(
                format!("/api/accounts/{}", path_segment(&account_id)),
                "PATCH",
                input,
            ),
            Self::AccountDelete { account_id } => (
                format!("/api/accounts/{}", path_segment(&account_id)),
                "DELETE",
                None,
            ),
            Self::AccountCredentialUpdate { account_id, input } => json_request(
                format!("/api/accounts/{}/credential", path_segment(&account_id)),
                "PUT",
                input,
            ),
            Self::AccountProxyUpdate { account_id, input } => json_request(
                format!("/api/accounts/{}/proxy", path_segment(&account_id)),
                "PUT",
                input,
            ),
            Self::AccountConnectionTest { account_id } => (
                format!(
                    "/api/accounts/{}/connection-test",
                    path_segment(&account_id)
                ),
                "POST",
                None,
            ),
            Self::AppleHmeStatus { account_id } => (
                format!("/api/accounts/{}/apple-hme", path_segment(&account_id)),
                "GET",
                None,
            ),
            Self::AppleHmeStartLogin { account_id, input } => json_request(
                format!(
                    "/api/accounts/{}/apple-hme/login",
                    path_segment(&account_id)
                ),
                "POST",
                input,
            ),
            Self::AppleHmeSubmitTwoFactor { account_id, input } => json_request(
                format!(
                    "/api/accounts/{}/apple-hme/two-factor",
                    path_segment(&account_id)
                ),
                "POST",
                input,
            ),
            Self::AppleHmeList { account_id } => (
                format!(
                    "/api/accounts/{}/apple-hme/addresses",
                    path_segment(&account_id)
                ),
                "GET",
                None,
            ),
            Self::AppleHmeSync { account_id } => (
                format!(
                    "/api/accounts/{}/apple-hme/addresses/sync",
                    path_segment(&account_id)
                ),
                "POST",
                None,
            ),
            Self::AppleHmeCreate { account_id, input } => json_request(
                format!(
                    "/api/accounts/{}/apple-hme/addresses",
                    path_segment(&account_id)
                ),
                "POST",
                input,
            ),
            Self::AppleHmeDeactivate {
                account_id,
                anonymous_id,
            } => (
                format!(
                    "/api/accounts/{}/apple-hme/addresses/{}/deactivate",
                    path_segment(&account_id),
                    path_segment(&anonymous_id)
                ),
                "POST",
                None,
            ),
            Self::AppleHmeDelete {
                account_id,
                anonymous_id,
            } => (
                format!(
                    "/api/accounts/{}/apple-hme/addresses/{}",
                    path_segment(&account_id),
                    path_segment(&anonymous_id)
                ),
                "DELETE",
                None,
            ),
            Self::AppleHmeDisconnect { account_id } => (
                format!("/api/accounts/{}/apple-hme", path_segment(&account_id)),
                "DELETE",
                None,
            ),
            Self::OauthStart { input } => json_request("/api/oauth/start", "POST", input),
            Self::OauthReconnect { account_id } => (
                format!(
                    "/api/accounts/{}/oauth/reconnect",
                    path_segment(&account_id)
                ),
                "POST",
                None,
            ),
            Self::OauthStatus { input } => json_request("/api/oauth/status", "POST", input),
            Self::MessageStats => ("/api/message-stats".into(), "GET", None),
            Self::MessagesList { query } => {
                let encoded = url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(query)
                    .finish();
                let path = if encoded.is_empty() {
                    "/api/messages".into()
                } else {
                    format!("/api/messages?{encoded}")
                };
                (path, "GET", None)
            }
            Self::MessageDetail { message_id } => (
                format!("/api/messages/{}", path_segment(&message_id)),
                "GET",
                None,
            ),
            Self::MessageSource { message_id } => (
                format!("/api/messages/{}/source", path_segment(&message_id)),
                "GET",
                None,
            ),
            Self::MessageConversation { message_id } => (
                format!("/api/messages/{}/conversation", path_segment(&message_id)),
                "GET",
                None,
            ),
            Self::SyncAll => ("/api/sync".into(), "POST", None),
            Self::SyncAccount { account_id } => (
                format!("/api/accounts/{}/sync", path_segment(&account_id)),
                "POST",
                None,
            ),
            Self::SyncAccountMailbox {
                account_id,
                mailbox,
            } => (
                format!("/api/accounts/{}/mailboxes/sync", path_segment(&account_id)),
                "POST",
                Some(serde_json::json!({"mailbox": mailbox}).to_string()),
            ),
            Self::SyncMailboxRole { role } => (
                format!("/api/mailboxes/{}/sync", path_segment(&role)),
                "POST",
                None,
            ),
            Self::MessageUpdate {
                message_id,
                unread,
                flagged,
                labels,
                snoozed_until,
            } => (
                format!("/api/messages/{}", path_segment(&message_id)),
                "PATCH",
                Some(
                    serde_json::json!({
                        "unread": unread,
                        "flagged": flagged,
                        "labels": labels,
                        "snoozedUntil": snoozed_until,
                    })
                    .to_string(),
                ),
            ),
            Self::MessageMove {
                message_id,
                destination,
            } => (
                format!("/api/messages/{}/move", path_segment(&message_id)),
                "POST",
                Some(serde_json::json!({"destination": destination}).to_string()),
            ),
            Self::AttachmentPreviewCreate { message_id, index } => (
                format!(
                    "/api/messages/{}/attachments/{index}/preview",
                    path_segment(&message_id)
                ),
                "POST",
                None,
            ),
            Self::AttachmentPreviewDelete { preview_id } => (
                format!("/api/attachment-previews/{}", path_segment(&preview_id)),
                "DELETE",
                None,
            ),
            Self::LabelsList => ("/api/labels".into(), "GET", None),
            Self::ContactsList => ("/api/contacts".into(), "GET", None),
            Self::NotificationsList => ("/api/notifications".into(), "GET", None),
            Self::MessageSend { input } => json_request("/api/send", "POST", input),
            Self::OutboxList => ("/api/outbox".into(), "GET", None),
            Self::OutboxSchedule { input } => json_request("/api/outbox", "POST", input),
            Self::OutboxCancel { item_id } => (
                format!("/api/outbox/{}", path_segment(&item_id)),
                "DELETE",
                None,
            ),
            Self::OutboxRetry { item_id } => (
                format!("/api/outbox/{}/retry", path_segment(&item_id)),
                "POST",
                None,
            ),
            Self::OutboxResolve { item_id, input } => json_request(
                format!("/api/outbox/{}/resolve", path_segment(&item_id)),
                "POST",
                input,
            ),
            Self::MailWorkItemsList => ("/api/mail-work-items".into(), "GET", None),
            Self::MailWorkItemSet { message_id, input } => json_request(
                format!("/api/messages/{}/work-item", path_segment(&message_id)),
                "PUT",
                input,
            ),
            Self::MailWorkItemComplete { message_id } => (
                format!("/api/messages/{}/work-item", path_segment(&message_id)),
                "DELETE",
                None,
            ),
            Self::MailReplyDraftCreate { message_id, input } => json_request(
                format!("/api/messages/{}/reply-draft", path_segment(&message_id)),
                "POST",
                input,
            ),
            Self::MailDraftSchedule { draft_id, input } => json_request(
                format!("/api/drafts/{}/schedule", path_segment(&draft_id)),
                "POST",
                input,
            ),
            Self::DraftsList => ("/api/drafts".into(), "GET", None),
            Self::DraftCreate {
                draft_id: requested_id,
                input,
            } => {
                draft_id = requested_id;
                json_request("/api/drafts", "POST", input)
            }
            Self::DraftUpdate { draft_id, input } => json_request(
                format!("/api/drafts/{}", path_segment(&draft_id)),
                "PUT",
                input,
            ),
            Self::DraftDelete { draft_id } => (
                format!("/api/drafts/{}", path_segment(&draft_id)),
                "DELETE",
                None,
            ),
            Self::PreferencesGet => ("/api/preferences".into(), "GET", None),
            Self::SmartFoldersList => ("/api/smart-folders".into(), "GET", None),
            Self::MailRulesList => ("/api/mail-rules".into(), "GET", None),
            Self::MailRuleGet { rule_id } => (
                format!("/api/mail-rules/{}", path_segment(&rule_id)),
                "GET",
                None,
            ),
            Self::MailRuleCreate { input } => json_request("/api/mail-rules", "POST", input),
            Self::MailRuleUpdate { rule_id, input } => json_request(
                format!("/api/mail-rules/{}", path_segment(&rule_id)),
                "PUT",
                input,
            ),
            Self::MailRuleSetEnabled { rule_id, input } => json_request(
                format!("/api/mail-rules/{}/enabled", path_segment(&rule_id)),
                "PATCH",
                input,
            ),
            Self::MailRuleDelete { rule_id } => (
                format!("/api/mail-rules/{}", path_segment(&rule_id)),
                "DELETE",
                None,
            ),
            Self::MailRulePreview { input } => {
                json_request("/api/mail-rules/preview", "POST", input)
            }
            Self::MailRuleApply { input } => json_request("/api/mail-rules/apply", "POST", input),
            Self::MailRuleRuns => ("/api/mail-rule-runs".into(), "GET", None),
            Self::MailRuleRetry { run_id } => (
                format!("/api/mail-rule-runs/{}/retry", path_segment(&run_id)),
                "POST",
                None,
            ),
            Self::SmartFolderCreate { input } => json_request("/api/smart-folders", "POST", input),
            Self::SmartFolderUpdate { folder_id, input } => json_request(
                format!("/api/smart-folders/{}", path_segment(&folder_id)),
                "PUT",
                input,
            ),
            Self::SmartFolderDelete { folder_id } => (
                format!("/api/smart-folders/{}", path_segment(&folder_id)),
                "DELETE",
                None,
            ),
            Self::PreferencesUpdate { input } => json_request("/api/preferences", "PATCH", input),
            Self::TranslationSettingsGet => ("/api/translation-settings".into(), "GET", None),
            Self::TranslationSettingsUpdate { input } => {
                json_request("/api/translation-settings", "PUT", input)
            }
            Self::TranslationProfileUpsert { profile_id, input } => json_request(
                format!("/api/translation-profiles/{}", path_segment(&profile_id)),
                "PUT",
                input,
            ),
            Self::TranslationProfileDelete { profile_id } => (
                format!("/api/translation-profiles/{}", path_segment(&profile_id)),
                "DELETE",
                None,
            ),
            Self::TranslationCredentialUpdate { profile_id, input } => json_request(
                format!(
                    "/api/translation-profiles/{}/credential",
                    path_segment(&profile_id)
                ),
                "PUT",
                input,
            ),
            Self::TranslationCredentialClear { profile_id } => (
                format!(
                    "/api/translation-profiles/{}/credential",
                    path_segment(&profile_id)
                ),
                "DELETE",
                None,
            ),
            Self::TranslationConsentAccept { profile_id } => (
                format!(
                    "/api/translation-profiles/{}/consent",
                    path_segment(&profile_id)
                ),
                "POST",
                None,
            ),
            Self::TranslationConsentRevoke { profile_id } => (
                format!(
                    "/api/translation-profiles/{}/consent",
                    path_segment(&profile_id)
                ),
                "DELETE",
                None,
            ),
            Self::TranslationPrepare { message_id, input } => json_request(
                format!(
                    "/api/messages/{}/translations/prepare",
                    path_segment(&message_id)
                ),
                "POST",
                input,
            ),
            Self::TranslationExecute { message_id, input } => json_request(
                format!(
                    "/api/messages/{}/translations/run",
                    path_segment(&message_id)
                ),
                "POST",
                input,
            ),
            Self::TranslationComplete { message_id, input } => json_request(
                format!(
                    "/api/messages/{}/translations/complete",
                    path_segment(&message_id)
                ),
                "POST",
                input,
            ),
            Self::TranslationCacheClear => ("/api/translation-cache".into(), "DELETE", None),
            Self::DeveloperTokensList => ("/api/developer-tokens".into(), "GET", None),
            Self::DeveloperTokenCreate { input } => {
                json_request("/api/developer-tokens", "POST", input)
            }
            Self::DeveloperTokenDelete { token_id } => (
                format!("/api/developer-tokens/{}", path_segment(&token_id)),
                "DELETE",
                None,
            ),
            Self::ExternalAccessGet => ("/api/external-access".into(), "GET", None),
            Self::ExternalAccessUpdate { input } => {
                json_request("/api/external-access", "PATCH", input)
            }
            Self::AuthorizationExportPrepare { input } => {
                json_request("/api/security/mail-authorization-exports", "POST", input)
            }
            Self::UserDataClear { input } => {
                json_request("/api/security/clear-user-data", "POST", input)
            }
        };
        EmbeddedInvocation {
            request: EmbeddedServiceRequest {
                path,
                method: method.into(),
                body,
            },
            draft_id,
        }
    }
}

#[cfg(test)]
fn json_request(
    path: impl Into<String>,
    method: &'static str,
    input: serde_json::Value,
) -> (String, &'static str, Option<String>) {
    (path.into(), method, Some(input.to_string()))
}

#[cfg(test)]
fn path_segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn decode_path_segment(value: &str) -> Result<String, String> {
    url::form_urlencoded::parse(format!("value={value}").as_bytes())
        .find(|(key, _)| key == "value")
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| "嵌入式路径参数无效".to_string())
}

fn json_response(status: u16, body: serde_json::Value) -> EmbeddedServiceResponse {
    EmbeddedServiceResponse {
        status,
        body: body.to_string(),
    }
}

fn account_application_response(
    result: Result<
        imail_protocol::AccountReadModel,
        ApplicationError<imail_storage_sqlite::AuthStoreError>,
    >,
) -> EmbeddedServiceResponse {
    match result {
        Ok(account) => json_response(200, imail_http::accounts::embedded_account(account)),
        Err(ApplicationError::Domain {
            status, message, ..
        }) if status < 500 => json_response(status, serde_json::json!({"error":message})),
        Err(_) => json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"})),
    }
}

fn embedded_operation_response(
    result: Result<serde_json::Value, imail_http::EmbeddedOperationError>,
) -> EmbeddedServiceResponse {
    match result {
        Ok(body) => json_response(200, body),
        Err(error) => json_response(error.status, serde_json::json!({"error":error.message})),
    }
}

fn unauthorized_response() -> EmbeddedServiceResponse {
    json_response(401, serde_json::json!({"error":"请先登录"}))
}

fn timestamp_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExternalAccessInput {
    gateway_enabled: Option<bool>,
    mcp_enabled: Option<bool>,
}

pub struct EmbeddedMailServiceState {
    host: OnceCell<EmbeddedServiceHost>,
    switch_root: OnceCell<PathBuf>,
    session_loaded: OnceCell<()>,
    data_dir: Mutex<Option<PathBuf>>,
    session: Mutex<Option<HeaderValue>>,
    user_id: Mutex<Option<String>>,
    event_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    external_http: Mutex<Option<EmbeddedHttpAdapter>>,
    request_cancellations: RequestCancellationRegistry,
}

struct EmbeddedHttpAdapter {
    base_url: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Default for EmbeddedMailServiceState {
    fn default() -> Self {
        Self {
            host: OnceCell::new(),
            switch_root: OnceCell::new(),
            session_loaded: OnceCell::new(),
            data_dir: Mutex::new(None),
            session: Mutex::new(None),
            user_id: Mutex::new(None),
            event_task: Mutex::new(None),
            external_http: Mutex::new(None),
            request_cancellations: RequestCancellationRegistry::default(),
        }
    }
}

impl EmbeddedMailServiceState {
    fn select_data_dir(&self, value: PathBuf) -> Result<PathBuf, String> {
        let mut selected = self.data_dir.lock().map_err(|_| "嵌入式服务状态锁已损坏")?;
        if let Some(existing) = selected.as_ref() {
            if existing != &value {
                return Err("嵌入式服务已绑定其他数据目录".into());
            }
        } else {
            *selected = Some(value.clone());
        }
        Ok(value)
    }

    #[cfg(test)]
    async fn request(
        &self,
        data_dir: PathBuf,
        input: EmbeddedServiceRequest,
    ) -> Result<EmbeddedServiceResponse, String> {
        self.request_internal(data_dir, input, None).await
    }

    #[cfg(test)]
    async fn request_internal(
        &self,
        data_dir: PathBuf,
        input: EmbeddedServiceRequest,
        draft_id: Option<String>,
    ) -> Result<EmbeddedServiceResponse, String> {
        validate_request(&input)?;
        let data_dir = self.initialize(data_dir).await?;
        let host = self.host.get().expect("embedded host initialized");
        let router = host.router();
        let method = Method::from_bytes(input.method.as_bytes())
            .map_err(|_| "嵌入式服务请求方法无效".to_string())?;
        let mut request = Request::builder()
            .method(method)
            .uri(&input.path)
            .header(HOST, "localhost")
            .header("content-type", "application/json");
        if let Some(session) = self
            .session
            .lock()
            .map_err(|_| "嵌入式登录状态锁已损坏")?
            .clone()
        {
            request = request.header(COOKIE, session);
        }
        if let Some(draft_id) = draft_id {
            request = request.header("x-draft-id", draft_id);
        }
        let request = request
            .body(Body::from(input.body.unwrap_or_default()))
            .map_err(|_| "嵌入式服务请求无效".to_string())?;
        let response = match router.clone().oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        };
        let status = response.status().as_u16();
        let session_changed = self.capture_session(response.headers())?;
        if session_changed {
            self.persist_session(&data_dir)?;
            self.refresh_user_context(&data_dir).await?;
        } else if status == 401 {
            *self.user_id.lock().map_err(|_| "嵌入式用户状态锁已损坏")? = None;
        }
        let body = to_bytes(response.into_body(), MAX_REQUEST_BYTES)
            .await
            .map_err(|_| "嵌入式服务响应读取失败".to_string())?;
        let body = String::from_utf8(body.to_vec())
            .map_err(|_| "嵌入式服务返回了非 UTF-8 响应".to_string())?;
        Ok(EmbeddedServiceResponse { status, body })
    }

    async fn initialize(&self, data_dir: PathBuf) -> Result<PathBuf, String> {
        let data_dir = self.select_data_dir(data_dir)?;
        let host_data_dir = data_dir.clone();
        self.host
            .get_or_try_init(|| async move {
                prepare_empty_data_dir(&host_data_dir)?;
                self.load_session(&host_data_dir).await?;
                let mut config = HttpAdapterConfig::production(host_data_dir)
                    .with_sync_worker(true)
                    .with_oauth_environment(desktop_oauth_environment());
                config.gateway = true;
                config.mcp = true;
                EmbeddedServiceHost::start(config)
                    .map_err(|error| format!("初始化嵌入式 Rust 服务失败：{error}"))
            })
            .await?;
        Ok(data_dir)
    }

    async fn start_external_http(&self) -> Result<EmbeddedHttpEndpoint, String> {
        if let Some(adapter) = self
            .external_http
            .lock()
            .map_err(|_| "嵌入式 HTTP Adapter 状态锁已损坏")?
            .as_ref()
        {
            return Ok(EmbeddedHttpEndpoint {
                base_url: adapter.base_url.clone(),
            });
        }

        let data_dir = self
            .data_dir
            .lock()
            .map_err(|_| "嵌入式服务状态锁已损坏")?
            .clone()
            .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
        let listener = bind_external_http_listener(&data_dir).await?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("读取嵌入式 HTTP Adapter 地址失败：{error}"))?;
        if let Err(error) = persist_external_http_port(&data_dir, address.port()) {
            log::warn!(target: "desktop", "[embedded.http.port_persist_failed] {error}");
        }
        let base_url = format!(
            "http://{}:{}",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            address.port()
        );
        let router = self
            .host
            .get()
            .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?
            .router();
        let (shutdown, receiver) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let result = axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = receiver.await;
            })
            .await;
            if let Err(error) = result {
                log::error!(target: "desktop", "[embedded.http.failed] {error}");
            }
        });
        log::info!(target: "desktop", "[embedded.http.started] address={base_url}");
        *self
            .external_http
            .lock()
            .map_err(|_| "嵌入式 HTTP Adapter 状态锁已损坏")? = Some(EmbeddedHttpAdapter {
            base_url: base_url.clone(),
            shutdown: Some(shutdown),
            task,
        });
        Ok(EmbeddedHttpEndpoint { base_url })
    }

    async fn direct_call(
        &self,
        call: &EmbeddedDomainCall,
    ) -> Result<Option<EmbeddedServiceResponse>, String> {
        let data_dir = self
            .data_dir
            .lock()
            .map_err(|_| "嵌入式服务状态锁已损坏")?
            .clone()
            .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
        let database = data_dir.join("imail.sqlite");
        match call {
            EmbeddedDomainCall::SystemInfo => {
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                Ok(Some(json_response(200, host.service_info())))
            }
            EmbeddedDomainCall::Providers => {
                if self.current_user_id()?.is_none() {
                    return Ok(Some(unauthorized_response()));
                }
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "iMail 服务尚未初始化".to_string())?;
                Ok(Some(json_response(200, host.providers())))
            }
            EmbeddedDomainCall::AuthStatus => {
                let raw_session = self.raw_session()?;
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("读取认证状态失败：{error}"))?;
                    Ok::<_, String>(
                        AuthenticationService::new(&mut store)
                            .status(raw_session.as_deref(), false),
                    )
                })
                .await
                .map_err(|_| "认证状态任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(body) => json_response(
                        200,
                        serde_json::to_value(body).expect("authentication status serializes"),
                    ),
                    Err(_) => {
                        json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"}))
                    }
                }))
            }
            EmbeddedDomainCall::AuthRegister { input } => {
                let input = serde_json::from_value::<RegistrationInput>(input.clone()).ok();
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("注册失败：{error}"))?;
                    Ok::<_, String>(AuthenticationService::new(&mut store).register(
                        input,
                        "tauri-embedded",
                        false,
                    ))
                })
                .await
                .map_err(|_| "注册任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(authenticated) => {
                        self.set_authenticated_session(
                            &data_dir,
                            &authenticated.raw_session,
                            &authenticated.user.id,
                        )?;
                        json_response(201, serde_json::json!({"user":authenticated.user}))
                    }
                    Err(AuthenticationError::Domain {
                        status, message, ..
                    }) => json_response(status, serde_json::json!({"error":message})),
                    Err(AuthenticationError::Limited { .. }) => {
                        json_response(429, serde_json::json!({"error":"尝试过多，请稍后再试"}))
                    }
                    Err(AuthenticationError::Repository(error)) if error.is_unique_violation() => {
                        json_response(409, serde_json::json!({"error":"这个登录名已存在"}))
                    }
                    Err(AuthenticationError::Repository(_)) => {
                        json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"}))
                    }
                }))
            }
            EmbeddedDomainCall::AuthLogin { input } => {
                let input = serde_json::from_value::<LoginInput>(input.clone()).ok();
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("登录失败：{error}"))?;
                    Ok::<_, String>(
                        AuthenticationService::new(&mut store).login(input, "tauri-embedded"),
                    )
                })
                .await
                .map_err(|_| "登录任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(authenticated) => {
                        self.set_authenticated_session(
                            &data_dir,
                            &authenticated.raw_session,
                            &authenticated.user.id,
                        )?;
                        json_response(200, serde_json::json!({"user":authenticated.user}))
                    }
                    Err(AuthenticationError::Domain {
                        status, message, ..
                    }) => json_response(status, serde_json::json!({"error":message})),
                    Err(AuthenticationError::Limited { .. }) => {
                        json_response(429, serde_json::json!({"error":"尝试过多，请稍后再试"}))
                    }
                    Err(AuthenticationError::Repository(_)) => {
                        json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"}))
                    }
                }))
            }
            EmbeddedDomainCall::AuthLogout => {
                let raw_session = self.raw_session()?;
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("退出登录失败：{error}"))?;
                    Ok::<_, String>(
                        AuthenticationService::new(&mut store)
                            .logout(raw_session.as_deref(), "tauri-embedded"),
                    )
                })
                .await
                .map_err(|_| "退出登录任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(()) => {
                        self.clear_authenticated_session(&data_dir)?;
                        EmbeddedServiceResponse {
                            status: 204,
                            body: String::new(),
                        }
                    }
                    Err(_) => {
                        json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"}))
                    }
                }))
            }
            EmbeddedDomainCall::AccountsList => {
                let user_id = self
                    .user_id
                    .lock()
                    .map_err(|_| "嵌入式用户状态锁已损坏")?
                    .clone();
                let Some(user_id) = user_id else {
                    return Ok(Some(json_response(
                        401,
                        serde_json::json!({"error":"请先登录"}),
                    )));
                };
                let body = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("读取本地数据失败：{error}"))?;
                    let values = AccountService::new(&mut store)
                        .list(&user_id)
                        .map_err(|error| format!("读取账户失败：{error}"))?;
                    Ok::<_, String>(imail_http::accounts::embedded_accounts(values))
                })
                .await
                .map_err(|_| "本地读取任务失败".to_string())??;
                Ok(Some(json_response(200, body)))
            }
            EmbeddedDomainCall::OutboxList
            | EmbeddedDomainCall::OutboxSchedule { .. }
            | EmbeddedDomainCall::OutboxCancel { .. }
            | EmbeddedDomainCall::OutboxRetry { .. }
            | EmbeddedDomainCall::OutboxResolve { .. } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let (operation, id, input, success_status) = match call {
                    EmbeddedDomainCall::OutboxList => ("list", None, None, 200),
                    EmbeddedDomainCall::OutboxSchedule { input } => {
                        ("schedule", None, Some(input.clone()), 201)
                    }
                    EmbeddedDomainCall::OutboxCancel { item_id } => {
                        ("cancel", Some(item_id.clone()), None, 200)
                    }
                    EmbeddedDomainCall::OutboxRetry { item_id } => {
                        ("retry", Some(item_id.clone()), None, 200)
                    }
                    EmbeddedDomainCall::OutboxResolve { item_id, input } => {
                        ("resolve", Some(item_id.clone()), Some(input.clone()), 200)
                    }
                    _ => unreachable!(),
                };
                let response = match host.outbox_operation(user_id, operation, id, input).await {
                    Ok(value) => json_response(success_status, value),
                    Err(error) => {
                        json_response(error.status, serde_json::json!({"error":error.message}))
                    }
                };
                Ok(Some(response))
            }
            EmbeddedDomainCall::MailWorkItemsList
            | EmbeddedDomainCall::MailWorkItemSet { .. }
            | EmbeddedDomainCall::MailWorkItemComplete { .. }
            | EmbeddedDomainCall::MailReplyDraftCreate { .. }
            | EmbeddedDomainCall::MailDraftSchedule { .. } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let (operation, id, input, success_status) = match call {
                    EmbeddedDomainCall::MailWorkItemsList => ("list", None, None, 200),
                    EmbeddedDomainCall::MailWorkItemSet { message_id, input } => {
                        ("set", Some(message_id.clone()), Some(input.clone()), 200)
                    }
                    EmbeddedDomainCall::MailWorkItemComplete { message_id } => {
                        ("complete", Some(message_id.clone()), None, 200)
                    }
                    EmbeddedDomainCall::MailReplyDraftCreate { message_id, input } => (
                        "replyDraft",
                        Some(message_id.clone()),
                        Some(input.clone()),
                        201,
                    ),
                    EmbeddedDomainCall::MailDraftSchedule { draft_id, input } => (
                        "scheduleDraft",
                        Some(draft_id.clone()),
                        Some(input.clone()),
                        201,
                    ),
                    _ => unreachable!(),
                };
                let response = match host
                    .work_queue_operation(user_id, operation, id, input)
                    .await
                {
                    Ok(value) => json_response(success_status, value),
                    Err(error) => {
                        json_response(error.status, serde_json::json!({"error":error.message}))
                    }
                };
                Ok(Some(response))
            }
            EmbeddedDomainCall::DraftsList => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let body = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("读取草稿失败：{error}"))?;
                    DraftService::new(&mut store)
                        .list(&user_id)
                        .map(|drafts| serde_json::json!({"drafts":drafts}))
                        .map_err(|error| format!("读取草稿失败：{error}"))
                })
                .await
                .map_err(|_| "草稿读取任务失败".to_string())??;
                Ok(Some(json_response(200, body)))
            }
            EmbeddedDomainCall::AccountUpdate { account_id, input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let patch = match serde_json::from_value::<AccountMetadataPatch>(input.clone()) {
                    Ok(patch) => patch,
                    Err(_) => {
                        return Ok(Some(json_response(
                            400,
                            serde_json::json!({"error":"请求参数无效"}),
                        )))
                    }
                };
                let account_id = account_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("更新账户失败：{error}"))?;
                    Ok::<_, String>(AccountService::new(&mut store).update_metadata(
                        &user_id,
                        &account_id,
                        patch,
                    ))
                })
                .await
                .map_err(|_| "账户更新任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(account) => {
                        json_response(200, imail_http::accounts::embedded_account(account))
                    }
                    Err(ApplicationError::Domain {
                        status, message, ..
                    }) if status < 500 => {
                        json_response(status, serde_json::json!({"error":message}))
                    }
                    Err(_) => {
                        json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"}))
                    }
                }))
            }
            EmbeddedDomainCall::AccountDelete { account_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let account_id = account_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("删除账户失败：{error}"))?;
                    Ok::<_, String>(AccountService::new(&mut store).remove_with_audit(
                        &user_id,
                        "tauri-embedded",
                        &account_id,
                    ))
                })
                .await
                .map_err(|_| "账户删除任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(_) => EmbeddedServiceResponse {
                        status: 204,
                        body: String::new(),
                    },
                    Err(ApplicationError::Domain {
                        status, message, ..
                    }) if status < 500 => {
                        json_response(status, serde_json::json!({"error":message}))
                    }
                    Err(_) => {
                        json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"}))
                    }
                }))
            }
            EmbeddedDomainCall::AccountCreate { input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host
                    .create_account(user_id, "tauri-embedded".into(), input.clone())
                    .await;
                let mut response = account_application_response(result);
                if response.status == 200 {
                    response.status = 201;
                }
                Ok(Some(response))
            }
            EmbeddedDomainCall::AccountCredentialUpdate { account_id, input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let Some(password) =
                    imail_http::accounts::embedded_credential_password(input.clone())
                else {
                    return Ok(Some(json_response(
                        400,
                        serde_json::json!({"error":"请求参数无效"}),
                    )));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host
                    .update_account_credential(
                        user_id,
                        "tauri-embedded".into(),
                        account_id.clone(),
                        password,
                    )
                    .await;
                Ok(Some(account_application_response(result)))
            }
            EmbeddedDomainCall::AccountProxyUpdate { account_id, input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let Some(input) = imail_http::accounts::embedded_proxy_update(input.clone()) else {
                    return Ok(Some(json_response(
                        400,
                        serde_json::json!({"error":"请求参数无效"}),
                    )));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host
                    .update_account_proxy(
                        user_id,
                        "tauri-embedded".into(),
                        account_id.clone(),
                        input,
                    )
                    .await;
                Ok(Some(account_application_response(result)))
            }
            EmbeddedDomainCall::AccountConnectionTest { account_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host
                    .test_account_connection(user_id, account_id.clone())
                    .await;
                Ok(Some(account_application_response(result)))
            }
            EmbeddedDomainCall::AppleHmeStatus { account_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "iMail 服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.apple_hme_status(user_id, account_id.clone()).await,
                )))
            }
            EmbeddedDomainCall::AppleHmeStartLogin { account_id, input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "iMail 服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.apple_hme_start_login(
                        user_id,
                        "imail-desktop".into(),
                        account_id.clone(),
                        input.clone(),
                    )
                    .await,
                )))
            }
            EmbeddedDomainCall::AppleHmeSubmitTwoFactor { account_id, input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "iMail 服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.apple_hme_submit_two_factor(
                        user_id,
                        "imail-desktop".into(),
                        account_id.clone(),
                        input.clone(),
                    )
                    .await,
                )))
            }
            EmbeddedDomainCall::AppleHmeList { account_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "iMail 服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.apple_hme_list(user_id, account_id.clone()).await,
                )))
            }
            EmbeddedDomainCall::AppleHmeSync { account_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "iMail 服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.apple_hme_sync(user_id, account_id.clone()).await,
                )))
            }
            EmbeddedDomainCall::AppleHmeCreate { account_id, input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "iMail 服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.apple_hme_create(user_id, account_id.clone(), input.clone())
                        .await,
                )))
            }
            EmbeddedDomainCall::AppleHmeDeactivate {
                account_id,
                anonymous_id,
            } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "iMail 服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.apple_hme_deactivate(user_id, account_id.clone(), anonymous_id.clone())
                        .await,
                )))
            }
            EmbeddedDomainCall::AppleHmeDelete {
                account_id,
                anonymous_id,
            } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "iMail 服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.apple_hme_delete(user_id, account_id.clone(), anonymous_id.clone())
                        .await,
                )))
            }
            EmbeddedDomainCall::AppleHmeDisconnect { account_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "iMail 服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.apple_hme_disconnect(user_id, account_id.clone()).await,
                )))
            }
            EmbeddedDomainCall::OauthStart { input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host
                    .start_oauth(user_id, "tauri-embedded".into(), input.clone())
                    .await;
                Ok(Some(embedded_operation_response(result)))
            }
            EmbeddedDomainCall::OauthReconnect { account_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host
                    .reconnect_oauth(user_id, "tauri-embedded".into(), account_id.clone())
                    .await;
                Ok(Some(embedded_operation_response(result)))
            }
            EmbeddedDomainCall::OauthStatus { input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host.oauth_status(user_id, input.clone()).await;
                Ok(Some(embedded_operation_response(result)))
            }
            EmbeddedDomainCall::SyncAll => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.sync_all(user_id, "inbox".into()).await,
                )))
            }
            EmbeddedDomainCall::SyncAccount { account_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.sync_account(user_id, account_id.clone(), "inbox".into(), None)
                        .await,
                )))
            }
            EmbeddedDomainCall::SyncAccountMailbox {
                account_id,
                mailbox,
            } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.sync_account(
                        user_id,
                        account_id.clone(),
                        "custom".into(),
                        Some(mailbox.clone()),
                    )
                    .await,
                )))
            }
            EmbeddedDomainCall::SyncMailboxRole { role } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.sync_all(user_id, role.clone()).await,
                )))
            }
            EmbeddedDomainCall::MailRulesList
            | EmbeddedDomainCall::MailRuleGet { .. }
            | EmbeddedDomainCall::MailRuleCreate { .. }
            | EmbeddedDomainCall::MailRuleUpdate { .. }
            | EmbeddedDomainCall::MailRuleSetEnabled { .. }
            | EmbeddedDomainCall::MailRuleDelete { .. }
            | EmbeddedDomainCall::MailRulePreview { .. }
            | EmbeddedDomainCall::MailRuleApply { .. }
            | EmbeddedDomainCall::MailRuleRuns
            | EmbeddedDomainCall::MailRuleRetry { .. } => {
                let Some(owner) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let (operation, id, input) = match call {
                    EmbeddedDomainCall::MailRulesList => ("list", None, None),
                    EmbeddedDomainCall::MailRuleGet { rule_id } => {
                        ("get", Some(rule_id.clone()), None)
                    }
                    EmbeddedDomainCall::MailRuleCreate { input } => {
                        ("create", None, Some(input.clone()))
                    }
                    EmbeddedDomainCall::MailRuleUpdate { rule_id, input } => {
                        ("update", Some(rule_id.clone()), Some(input.clone()))
                    }
                    EmbeddedDomainCall::MailRuleSetEnabled { rule_id, input } => {
                        ("set_enabled", Some(rule_id.clone()), Some(input.clone()))
                    }
                    EmbeddedDomainCall::MailRuleDelete { rule_id } => {
                        ("delete", Some(rule_id.clone()), None)
                    }
                    EmbeddedDomainCall::MailRulePreview { input } => {
                        ("preview", None, Some(input.clone()))
                    }
                    EmbeddedDomainCall::MailRuleApply { input } => {
                        ("apply", None, Some(input.clone()))
                    }
                    EmbeddedDomainCall::MailRuleRuns => ("runs", None, None),
                    EmbeddedDomainCall::MailRuleRetry { run_id } => {
                        ("retry", Some(run_id.clone()), None)
                    }
                    _ => unreachable!(),
                };
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(imail_core::ApplicationError::Repository)?;
                    imail_http::rules::execute(&mut store, &owner, operation, id.as_deref(), input)
                })
                .await
                .map_err(|_| "邮件规则任务失败".to_string())?;
                Ok(Some(match result {
                    Ok(value) => json_response(200, value),
                    Err(imail_core::ApplicationError::Domain {
                        status, message, ..
                    }) => json_response(status, serde_json::json!({"error":message})),
                    Err(_) => json_response(500, serde_json::json!({"error":"邮件规则操作失败"})),
                }))
            }
            EmbeddedDomainCall::SmartFoldersList
            | EmbeddedDomainCall::SmartFolderCreate { .. }
            | EmbeddedDomainCall::SmartFolderUpdate { .. }
            | EmbeddedDomainCall::SmartFolderDelete { .. } => {
                let Some(owner) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let (method, id, input) = match call {
                    EmbeddedDomainCall::SmartFoldersList => ("GET", None, None),
                    EmbeddedDomainCall::SmartFolderCreate { input } => {
                        ("POST", None, Some(input.clone()))
                    }
                    EmbeddedDomainCall::SmartFolderUpdate { folder_id, input } => {
                        ("PUT", Some(folder_id.clone()), Some(input.clone()))
                    }
                    EmbeddedDomainCall::SmartFolderDelete { folder_id } => {
                        ("DELETE", Some(folder_id.clone()), None)
                    }
                    _ => unreachable!(),
                };
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(imail_core::ApplicationError::Repository)?;
                    imail_http::search::execute(&mut store, &owner, method, id.as_deref(), input)
                })
                .await
                .map_err(|_| "智能文件夹任务失败".to_string())?;
                Ok(Some(match result {
                    Ok(value) => json_response(200, value),
                    Err(imail_core::ApplicationError::Domain {
                        status, message, ..
                    }) => json_response(status, serde_json::json!({"error":message})),
                    Err(_) => json_response(500, serde_json::json!({"error":"智能文件夹操作失败"})),
                }))
            }
            EmbeddedDomainCall::MessagesList { query } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let query = match imail_http::messages::embedded_message_query(query) {
                    Ok(query) => query,
                    Err(message) => {
                        return Ok(Some(json_response(
                            400,
                            serde_json::json!({"error":message}),
                        )))
                    }
                };
                let body = tokio::task::spawn_blocking(move || {
                    let store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("读取邮件失败：{error}"))?;
                    let page = MessageQueryService::new(&store)
                        .query(&user_id, &query, &timestamp_now())
                        .map_err(|error| format!("读取邮件失败：{error}"))?;
                    let contacts = store
                        .list_contacts(&user_id)
                        .map_err(|error| format!("读取联系人失败：{error}"))?;
                    Ok::<_, String>(imail_http::messages::embedded_message_page(
                        page, &query, &contacts,
                    ))
                })
                .await
                .map_err(|_| "邮件读取任务失败".to_string())??;
                Ok(Some(json_response(200, body)))
            }
            EmbeddedDomainCall::MessageDetail { message_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let message_id = message_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("读取邮件失败：{error}"))?;
                    match MessageQueryService::new(&store).get(&user_id, &message_id) {
                        Ok(message) => {
                            let contacts = store
                                .list_contacts(&user_id)
                                .map_err(|error| format!("读取联系人失败：{error}"))?;
                            Ok(Ok(imail_http::messages::embedded_message_detail(
                                message, &contacts,
                            )))
                        }
                        Err(ApplicationError::Domain {
                            status, message, ..
                        }) => Ok(Err((status, message))),
                        Err(error) => Err(format!("读取邮件失败：{error}")),
                    }
                })
                .await
                .map_err(|_| "邮件读取任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(body) => json_response(200, body),
                    Err((status, message)) => {
                        json_response(status, serde_json::json!({"error":message}))
                    }
                }))
            }
            EmbeddedDomainCall::MessageConversation { message_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let message_id = message_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("读取邮件失败：{error}"))?;
                    match MessageQueryService::new(&store).conversation(&user_id, &message_id) {
                        Ok(message) => {
                            let contacts = store
                                .list_contacts(&user_id)
                                .map_err(|error| format!("读取联系人失败：{error}"))?;
                            Ok(Ok(imail_http::messages::embedded_conversation(
                                message, &contacts,
                            )))
                        }
                        Err(ApplicationError::Domain {
                            status, message, ..
                        }) => Ok(Err((status, message))),
                        Err(error) => Err(format!("读取邮件失败：{error}")),
                    }
                })
                .await
                .map_err(|_| "邮件读取任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(body) => json_response(200, body),
                    Err((status, message)) => {
                        json_response(status, serde_json::json!({"error":message}))
                    }
                }))
            }
            EmbeddedDomainCall::MessageSource { message_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let message_id = message_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("读取邮件原始内容失败：{error}"))?;
                    match MessageQueryService::new(&store).source(&user_id, &message_id) {
                        Ok(source) => Ok(Ok(imail_http::messages::embedded_message_source(source))),
                        Err(ApplicationError::Domain {
                            status, message, ..
                        }) => Ok(Err((status, message))),
                        Err(error) => Err(format!("读取邮件原始内容失败：{error}")),
                    }
                })
                .await
                .map_err(|_| "邮件原始内容读取任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(body) => json_response(200, body),
                    Err((status, message)) => {
                        json_response(status, serde_json::json!({"error":message}))
                    }
                }))
            }
            EmbeddedDomainCall::MessageUpdate {
                message_id,
                unread,
                flagged,
                labels,
                snoozed_until,
            } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host
                    .update_message(
                        user_id,
                        message_id.clone(),
                        serde_json::json!({
                            "unread":unread,
                            "flagged":flagged,
                            "labels":labels,
                            "snoozedUntil":snoozed_until,
                        }),
                    )
                    .await;
                Ok(Some(embedded_operation_response(result)))
            }
            EmbeddedDomainCall::MessageMove {
                message_id,
                destination,
            } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host
                    .move_message(user_id, message_id.clone(), destination.clone())
                    .await;
                Ok(Some(embedded_operation_response(result)))
            }
            EmbeddedDomainCall::AttachmentPreviewCreate { message_id, index } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host
                    .create_attachment_preview(user_id, message_id.clone(), *index)
                    .await;
                let mut response = embedded_operation_response(result);
                if response.status == 200 {
                    response.status = 201;
                }
                Ok(Some(response))
            }
            EmbeddedDomainCall::AttachmentPreviewDelete { preview_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                Ok(Some(
                    match host.delete_attachment_preview(user_id, preview_id.clone()) {
                        Ok(()) => EmbeddedServiceResponse {
                            status: 204,
                            body: String::new(),
                        },
                        Err(error) => {
                            json_response(error.status, serde_json::json!({"error":error.message}))
                        }
                    },
                ))
            }
            EmbeddedDomainCall::MessageSend { input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host.send_message(user_id, input.clone()).await;
                let mut response = embedded_operation_response(result);
                if response.status == 200 {
                    response.status = 201;
                }
                Ok(Some(response))
            }
            EmbeddedDomainCall::ContactsList => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let body = tokio::task::spawn_blocking(move || {
                    let store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("读取联系人失败：{error}"))?;
                    store
                        .list_contacts(&user_id)
                        .map(imail_http::messages::embedded_contacts)
                        .map_err(|error| format!("读取联系人失败：{error}"))
                })
                .await
                .map_err(|_| "联系人读取任务失败".to_string())??;
                Ok(Some(json_response(200, body)))
            }
            EmbeddedDomainCall::DraftCreate { draft_id, input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let Some(input) = imail_http::drafts::embedded_draft_input(input.clone()) else {
                    return Ok(Some(json_response(
                        400,
                        serde_json::json!({"error":"请求参数无效"}),
                    )));
                };
                let draft_id = match draft_id {
                    Some(value) if uuid::Uuid::parse_str(value).is_ok() => value.clone(),
                    Some(_) => {
                        return Ok(Some(json_response(
                            400,
                            serde_json::json!({"error":"请求参数无效"}),
                        )))
                    }
                    None => uuid::Uuid::new_v4().to_string(),
                };
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("保存草稿失败：{error}"))?;
                    let now = timestamp_now();
                    Ok::<_, String>(
                        DraftService::new(&mut store).create(&user_id, &draft_id, &now, input),
                    )
                })
                .await
                .map_err(|_| "草稿保存任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(draft) => json_response(201, serde_json::json!({"draft":draft})),
                    Err(ApplicationError::Domain {
                        status, message, ..
                    }) if status < 500 => {
                        json_response(status, serde_json::json!({"error":message}))
                    }
                    Err(_) => {
                        json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"}))
                    }
                }))
            }
            EmbeddedDomainCall::DraftUpdate { draft_id, input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let Some(input) = imail_http::drafts::embedded_draft_input(input.clone()) else {
                    return Ok(Some(json_response(
                        400,
                        serde_json::json!({"error":"请求参数无效"}),
                    )));
                };
                let draft_id = draft_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("保存草稿失败：{error}"))?;
                    let now = timestamp_now();
                    Ok::<_, String>(
                        DraftService::new(&mut store)
                            .save_existing(&user_id, &draft_id, &now, input),
                    )
                })
                .await
                .map_err(|_| "草稿保存任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(draft) => json_response(200, serde_json::json!({"draft":draft})),
                    Err(ApplicationError::Domain {
                        status, message, ..
                    }) if status < 500 => {
                        json_response(status, serde_json::json!({"error":message}))
                    }
                    Err(_) => {
                        json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"}))
                    }
                }))
            }
            EmbeddedDomainCall::DraftDelete { draft_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let draft_id = draft_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("删除草稿失败：{error}"))?;
                    Ok::<_, String>(DraftService::new(&mut store).delete(&user_id, &draft_id))
                })
                .await
                .map_err(|_| "草稿删除任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(_) => EmbeddedServiceResponse {
                        status: 204,
                        body: String::new(),
                    },
                    Err(ApplicationError::Domain {
                        status, message, ..
                    }) if status < 500 => {
                        json_response(status, serde_json::json!({"error":message}))
                    }
                    Err(_) => {
                        json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"}))
                    }
                }))
            }
            EmbeddedDomainCall::PreferencesUpdate { input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let patch = match serde_json::from_value::<AppPreferencesPatch>(input.clone()) {
                    Ok(patch) => patch,
                    Err(_) => {
                        return Ok(Some(json_response(
                            400,
                            serde_json::json!({"error":"请求参数无效"}),
                        )))
                    }
                };
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("更新偏好失败：{error}"))?;
                    Ok::<_, String>(PreferencesService::new(&mut store).update(&user_id, patch))
                })
                .await
                .map_err(|_| "偏好更新任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(preferences) => {
                        json_response(200, serde_json::json!({"preferences":preferences}))
                    }
                    Err(ApplicationError::Domain {
                        status, message, ..
                    }) if status < 500 => {
                        json_response(status, serde_json::json!({"error":message}))
                    }
                    Err(_) => {
                        json_response(500, serde_json::json!({"error":"服务暂时无法完成请求"}))
                    }
                }))
            }
            EmbeddedDomainCall::TranslationSettingsGet
            | EmbeddedDomainCall::TranslationSettingsUpdate { .. }
            | EmbeddedDomainCall::TranslationProfileUpsert { .. }
            | EmbeddedDomainCall::TranslationProfileDelete { .. }
            | EmbeddedDomainCall::TranslationCredentialUpdate { .. }
            | EmbeddedDomainCall::TranslationCredentialClear { .. }
            | EmbeddedDomainCall::TranslationConsentAccept { .. }
            | EmbeddedDomainCall::TranslationConsentRevoke { .. }
            | EmbeddedDomainCall::TranslationPrepare { .. }
            | EmbeddedDomainCall::TranslationExecute { .. }
            | EmbeddedDomainCall::TranslationComplete { .. }
            | EmbeddedDomainCall::TranslationCacheClear => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                if matches!(
                    call,
                    EmbeddedDomainCall::TranslationPrepare { .. }
                        | EmbeddedDomainCall::TranslationExecute { .. }
                        | EmbeddedDomainCall::TranslationComplete { .. }
                        | EmbeddedDomainCall::TranslationCacheClear
                ) {
                    use imail_http::translations::{
                        embedded_call, TranslationApplicationCall as Call,
                    };
                    let application_call = match call {
                        EmbeddedDomainCall::TranslationPrepare { message_id, input } => {
                            let Ok(input) = serde_json::from_value(input.clone()) else {
                                return Ok(Some(json_response(
                                    400,
                                    serde_json::json!({"error":"请求参数无效"}),
                                )));
                            };
                            Call::Prepare {
                                message_id: message_id.clone(),
                                input,
                            }
                        }
                        EmbeddedDomainCall::TranslationExecute { message_id, input } => {
                            let Ok(input) = serde_json::from_value(input.clone()) else {
                                return Ok(Some(json_response(
                                    400,
                                    serde_json::json!({"error":"请求参数无效"}),
                                )));
                            };
                            Call::Execute {
                                message_id: message_id.clone(),
                                input,
                            }
                        }
                        EmbeddedDomainCall::TranslationComplete { message_id, input } => {
                            let Ok(input) = serde_json::from_value(input.clone()) else {
                                return Ok(Some(json_response(
                                    400,
                                    serde_json::json!({"error":"请求参数无效"}),
                                )));
                            };
                            Call::Complete {
                                message_id: message_id.clone(),
                                input,
                            }
                        }
                        EmbeddedDomainCall::TranslationCacheClear => Call::ClearCache,
                        _ => unreachable!(),
                    };
                    let response = embedded_call(data_dir, user_id, application_call).await;
                    return Ok(Some(json_response(response.status, response.body)));
                }
                use imail_http::translation_settings::TranslationSettingsApplicationCall as Call;
                let application_call = match call {
                    EmbeddedDomainCall::TranslationSettingsGet => Call::Read,
                    EmbeddedDomainCall::TranslationSettingsUpdate { input } => {
                        let Ok(input) =
                            serde_json::from_value::<TranslationSettingsUpdate>(input.clone())
                        else {
                            return Ok(Some(json_response(
                                400,
                                serde_json::json!({"error":"请求参数无效"}),
                            )));
                        };
                        Call::Update(input)
                    }
                    EmbeddedDomainCall::TranslationProfileUpsert { profile_id, input } => {
                        let Ok(input) = serde_json::from_value::<TranslationProviderProfileInput>(
                            input.clone(),
                        ) else {
                            return Ok(Some(json_response(
                                400,
                                serde_json::json!({"error":"请求参数无效"}),
                            )));
                        };
                        Call::UpsertProfile {
                            profile_id: profile_id.clone(),
                            input,
                        }
                    }
                    EmbeddedDomainCall::TranslationProfileDelete { profile_id } => {
                        Call::DeleteProfile {
                            profile_id: profile_id.clone(),
                        }
                    }
                    EmbeddedDomainCall::TranslationCredentialUpdate { profile_id, input } => {
                        let Ok(input) = serde_json::from_value(input.clone()) else {
                            return Ok(Some(json_response(
                                400,
                                serde_json::json!({"error":"请求参数无效"}),
                            )));
                        };
                        Call::UpdateCredential {
                            profile_id: profile_id.clone(),
                            input,
                        }
                    }
                    EmbeddedDomainCall::TranslationCredentialClear { profile_id } => {
                        Call::ClearCredential {
                            profile_id: profile_id.clone(),
                        }
                    }
                    EmbeddedDomainCall::TranslationConsentAccept { profile_id } => {
                        Call::AcceptConsent {
                            profile_id: profile_id.clone(),
                        }
                    }
                    EmbeddedDomainCall::TranslationConsentRevoke { profile_id } => {
                        Call::RevokeConsent {
                            profile_id: profile_id.clone(),
                        }
                    }
                    _ => unreachable!(),
                };
                let response = imail_http::translation_settings::embedded_call(
                    data_dir,
                    user_id,
                    application_call,
                )
                .await;
                Ok(Some(json_response(response.status, response.body)))
            }
            EmbeddedDomainCall::ExternalAccessUpdate { input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let input = match serde_json::from_value::<ExternalAccessInput>(input.clone()) {
                    Ok(input) => input,
                    Err(_) => {
                        return Ok(Some(json_response(
                            400,
                            serde_json::json!({"error":"请求参数无效"}),
                        )))
                    }
                };
                if input.gateway_enabled.is_none() && input.mcp_enabled.is_none() {
                    return Ok(Some(json_response(
                        400,
                        serde_json::json!({"error":"至少提供一个要更新的外部接入设置"}),
                    )));
                }
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("更新外部访问设置失败：{error}"))?;
                    Ok::<_, String>(ExternalAccessService::new(&mut store).update(
                        &user_id,
                        ExternalAccessChanges {
                            gateway_enabled: input.gateway_enabled,
                            mcp_enabled: input.mcp_enabled,
                        },
                    ))
                })
                .await
                .map_err(|_| "外部访问设置更新任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(settings) => json_response(200, serde_json::json!({"settings":settings})),
                    Err(_) => json_response(500, serde_json::json!({"error":"服务暂时不可用"})),
                }))
            }
            EmbeddedDomainCall::DeveloperTokensList => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("读取开发者令牌失败：{error}"))?;
                    Ok::<_, String>(DeveloperTokenService::new(&mut store).list(&user_id))
                })
                .await
                .map_err(|_| "开发者令牌读取任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(tokens) => json_response(200, serde_json::json!({"tokens":tokens})),
                    Err(_) => json_response(500, serde_json::json!({"error":"服务暂时不可用"})),
                }))
            }
            EmbeddedDomainCall::DeveloperTokenCreate { input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let input = match serde_json::from_value::<CreateDeveloperTokenInput>(input.clone())
                {
                    Ok(input) => input,
                    Err(_) => {
                        return Ok(Some(json_response(
                            400,
                            serde_json::json!({"error":"请求参数无效"}),
                        )))
                    }
                };
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("创建开发者令牌失败：{error}"))?;
                    Ok::<_, String>(DeveloperTokenService::new(&mut store).create(
                        &user_id,
                        "tauri-embedded",
                        input,
                    ))
                })
                .await
                .map_err(|_| "开发者令牌创建任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(issued) => json_response(
                        201,
                        serde_json::to_value(issued).expect("issued token serializes"),
                    ),
                    Err(ApplicationError::Domain {
                        status, message, ..
                    }) if status < 500 => {
                        json_response(status, serde_json::json!({"error":message}))
                    }
                    Err(_) => json_response(500, serde_json::json!({"error":"服务暂时不可用"})),
                }))
            }
            EmbeddedDomainCall::DeveloperTokenDelete { token_id } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let token_id = token_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("撤销开发者令牌失败：{error}"))?;
                    Ok::<_, String>(DeveloperTokenService::new(&mut store).revoke(
                        &user_id,
                        "tauri-embedded",
                        &token_id,
                    ))
                })
                .await
                .map_err(|_| "开发者令牌撤销任务失败".to_string())??;
                Ok(Some(match result {
                    Ok(()) => EmbeddedServiceResponse {
                        status: 204,
                        body: String::new(),
                    },
                    Err(_) => json_response(500, serde_json::json!({"error":"服务暂时不可用"})),
                }))
            }
            EmbeddedDomainCall::AuthorizationExportPrepare { input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                Ok(Some(embedded_operation_response(
                    host.prepare_authorization_export(
                        user_id,
                        "tauri-embedded".into(),
                        input.clone(),
                    )
                    .await,
                )))
            }
            EmbeddedDomainCall::UserDataClear { input } => {
                let Some(user_id) = self.current_user_id()? else {
                    return Ok(Some(unauthorized_response()));
                };
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
                let result = host
                    .clear_user_data(user_id, "tauri-embedded".into(), input.clone())
                    .await;
                Ok(Some(match result {
                    Ok(()) => EmbeddedServiceResponse {
                        status: 204,
                        body: String::new(),
                    },
                    Err(error) => {
                        json_response(error.status, serde_json::json!({"error":error.message}))
                    }
                }))
            }
            EmbeddedDomainCall::MessageStats
            | EmbeddedDomainCall::LabelsList
            | EmbeddedDomainCall::NotificationsList
            | EmbeddedDomainCall::PreferencesGet
            | EmbeddedDomainCall::ExternalAccessGet => {
                let user_id = self
                    .user_id
                    .lock()
                    .map_err(|_| "嵌入式用户状态锁已损坏")?
                    .clone();
                let Some(user_id) = user_id else {
                    return Ok(Some(json_response(
                        401,
                        serde_json::json!({"error":"请先登录"}),
                    )));
                };
                let operation = match call {
                    EmbeddedDomainCall::MessageStats => "messageStats",
                    EmbeddedDomainCall::LabelsList => "labelsList",
                    EmbeddedDomainCall::NotificationsList => "notificationsList",
                    EmbeddedDomainCall::PreferencesGet => "preferencesGet",
                    EmbeddedDomainCall::ExternalAccessGet => "externalAccessGet",
                    _ => unreachable!(),
                };
                let body = tokio::task::spawn_blocking(move || {
                    let mut store = SqliteAuthStore::open_database(database)
                        .map_err(|error| format!("读取本地数据失败：{error}"))?;
                    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
                    match operation {
                        "messageStats" => MessageQueryService::new(&store)
                            .stats(&user_id, &now)
                            .map(|value| serde_json::to_value(value).expect("stats serialize"))
                            .map_err(|error| format!("读取邮件统计失败：{error}")),
                        "labelsList" => MailOverviewService::new(&store)
                            .labels(&user_id)
                            .map(|labels| serde_json::json!({"labels": labels}))
                            .map_err(|error| format!("读取标签失败：{error}")),
                        "notificationsList" => MailOverviewService::new(&store)
                            .notifications(&user_id, 30, &now)
                            .map(
                                |notifications| serde_json::json!({"notifications": notifications}),
                            )
                            .map_err(|error| format!("读取通知失败：{error}")),
                        "preferencesGet" => PreferencesService::new(&mut store)
                            .read(&user_id)
                            .map(|preferences| serde_json::json!({"preferences": preferences}))
                            .map_err(|error| format!("读取偏好失败：{error}")),
                        "externalAccessGet" => ExternalAccessService::new(&mut store)
                            .get(&user_id)
                            .map(|settings| serde_json::json!({"settings": settings}))
                            .map_err(|error| format!("读取外部访问设置失败：{error}")),
                        _ => unreachable!(),
                    }
                })
                .await
                .map_err(|_| "本地读取任务失败".to_string())??;
                Ok(Some(json_response(200, body)))
            }
        }
    }

    async fn read_binary(&self, path: String) -> Result<Vec<u8>, String> {
        let input = EmbeddedServiceRequest {
            path,
            method: "GET".into(),
            body: None,
        };
        validate_request(&input)?;
        let host = self
            .host
            .get()
            .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
        const EXPORT_PREFIX: &str = "/api/security/mail-authorization-exports/";
        if let Some(id) = input.path.strip_prefix(EXPORT_PREFIX) {
            if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("嵌入式授权导出路径无效".into());
            }
            let user_id = self
                .current_user_id()?
                .ok_or_else(|| "请先登录".to_string())?;
            return host
                .download_authorization_export(user_id, "tauri-embedded".into(), id.to_string())
                .await
                .map_err(|error| format!("嵌入式授权导出读取失败：{}", error.status));
        }
        let parsed = url::Url::parse(&format!("http://localhost{}", input.path))
            .map_err(|_| "嵌入式二进制路径无效".to_string())?;
        let user_id = self
            .current_user_id()?
            .ok_or_else(|| "请先登录".to_string())?;
        let segments = parsed
            .path_segments()
            .map(|items| items.collect::<Vec<_>>());
        if let Some(["api", "messages", message_id, "source", "download"]) = segments.as_deref() {
            let message_id = decode_path_segment(message_id)?;
            return host
                .download_message_source(user_id, message_id)
                .await
                .map_err(|error| format!("嵌入式原始邮件读取失败：{}", error.status));
        }
        if let Some(["api", "messages", message_id, "attachments", index]) = segments.as_deref() {
            let message_id = decode_path_segment(message_id)?;
            let index = index
                .parse::<usize>()
                .map_err(|_| "嵌入式附件索引无效".to_string())?;
            return host
                .download_attachment(user_id, message_id, index)
                .await
                .map_err(|error| format!("嵌入式附件读取失败：{}", error.status));
        }
        if let Some(["api", "attachment-previews", preview_id, "content"]) = segments.as_deref() {
            let preview_id = decode_path_segment(preview_id)?;
            return host
                .read_attachment_preview(user_id, preview_id, None)
                .await
                .map_err(|error| format!("嵌入式附件预览读取失败：{}", error.status));
        }
        if let Some(["api", "attachment-previews", preview_id, "archive", "entries", entry_id]) =
            segments.as_deref()
        {
            let preview_id = decode_path_segment(preview_id)?;
            let entry_id = decode_path_segment(entry_id)?;
            return host
                .read_attachment_preview(user_id, preview_id, Some(entry_id))
                .await
                .map_err(|error| format!("嵌入式压缩包条目读取失败：{}", error.status));
        }
        if parsed.path() == "/api/contacts/logo" {
            let address = parsed
                .query_pairs()
                .find(|(key, _)| key == "address")
                .map(|(_, value)| value.into_owned())
                .ok_or_else(|| "嵌入式联系人 Logo 地址无效".to_string())?;
            return host
                .contact_logo(user_id, address)
                .await
                .map_err(|error| format!("嵌入式联系人 Logo 读取失败：{}", error.status))?
                .ok_or_else(|| "嵌入式联系人 Logo 不存在".to_string());
        }
        Err("嵌入式二进制路径未实现".into())
    }

    #[cfg(test)]
    fn capture_session(&self, headers: &HeaderMap) -> Result<bool, String> {
        let mut changed = false;
        for value in headers.get_all(SET_COOKIE) {
            let Ok(value) = value.to_str() else { continue };
            let Some(pair) = value.split(';').next() else {
                continue;
            };
            if !pair.trim_start().starts_with("imail_session=") {
                continue;
            }
            let mut session = self.session.lock().map_err(|_| "嵌入式登录状态锁已损坏")?;
            if pair.trim() == "imail_session=" {
                *session = None;
            } else {
                *session = Some(
                    HeaderValue::from_str(pair.trim())
                        .map_err(|_| "嵌入式服务返回了无效登录状态".to_string())?,
                );
            }
            changed = true;
        }
        Ok(changed)
    }

    async fn load_session(&self, data_dir: &Path) -> Result<(), String> {
        let session_file = embedded_session_path(data_dir)?;
        self.session_loaded
            .get_or_try_init(|| async {
                if let Ok(raw) = fs::read_to_string(&session_file) {
                    let raw = raw.trim();
                    if !raw.is_empty() && raw.len() <= 4096 && !raw.contains(['\r', '\n']) {
                        *self.session.lock().map_err(|_| "嵌入式登录状态锁已损坏")? = Some(
                            HeaderValue::from_str(&format!("imail_session={raw}"))
                                .map_err(|_| "嵌入式登录状态文件无效".to_string())?,
                        );
                    }
                } else if let Some(raw) = legacy_session_token(data_dir)? {
                    *self.session.lock().map_err(|_| "嵌入式登录状态锁已损坏")? = Some(
                        HeaderValue::from_str(&format!("imail_session={raw}"))
                            .map_err(|_| "旧登录状态文件无效".to_string())?,
                    );
                }
                Ok::<(), String>(())
            })
            .await?;
        let needs_user = self
            .user_id
            .lock()
            .map_err(|_| "嵌入式用户状态锁已损坏")?
            .is_none()
            && self.raw_session()?.is_some();
        if needs_user {
            self.refresh_user_context(data_dir).await?;
            if self.current_user_id()?.is_some() && !session_file.exists() {
                self.persist_session(data_dir)?;
            } else if self.current_user_id()?.is_none() {
                self.clear_authenticated_session(data_dir)?;
            }
        }
        Ok(())
    }

    fn persist_session(&self, data_dir: &Path) -> Result<(), String> {
        let path = embedded_session_path(data_dir)?;
        let raw = self
            .session
            .lock()
            .map_err(|_| "嵌入式登录状态锁已损坏")?
            .as_ref()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("imail_session="))
            .map(str::to_owned);
        match raw {
            Some(raw) => write_private_session(&path, raw.as_bytes()),
            None if path.exists() => {
                fs::remove_file(path).map_err(|error| format!("清除嵌入式登录状态失败：{error}"))
            }
            None => Ok(()),
        }
    }

    fn set_authenticated_session(
        &self,
        data_dir: &Path,
        raw_session: &str,
        user_id: &str,
    ) -> Result<(), String> {
        if raw_session.is_empty() || raw_session.len() > 4096 || raw_session.contains(['\r', '\n'])
        {
            return Err("嵌入式服务生成了无效登录状态".into());
        }
        let header = HeaderValue::from_str(&format!("imail_session={raw_session}"))
            .map_err(|_| "嵌入式服务生成了无效登录状态".to_string())?;
        *self.session.lock().map_err(|_| "嵌入式登录状态锁已损坏")? = Some(header);
        *self.user_id.lock().map_err(|_| "嵌入式用户状态锁已损坏")? = Some(user_id.to_string());
        self.persist_session(data_dir)
    }

    fn clear_authenticated_session(&self, data_dir: &Path) -> Result<(), String> {
        *self.session.lock().map_err(|_| "嵌入式登录状态锁已损坏")? = None;
        *self.user_id.lock().map_err(|_| "嵌入式用户状态锁已损坏")? = None;
        self.persist_session(data_dir)
    }

    async fn refresh_user_context(&self, data_dir: &Path) -> Result<(), String> {
        let raw_session = self.raw_session()?;
        let Some(raw_session) = raw_session else {
            *self.user_id.lock().map_err(|_| "嵌入式用户状态锁已损坏")? = None;
            return Ok(());
        };
        let database = data_dir.join("imail.sqlite");
        let user_id = tokio::task::spawn_blocking(move || {
            let mut auth = SqliteAuthStore::open_database(database)
                .map_err(|error| format!("读取嵌入式登录状态失败：{error}"))?;
            auth.user_for_session(&raw_session)
                .map(|user| user.map(|user| user.id))
                .map_err(|error| format!("读取嵌入式登录状态失败：{error}"))
        })
        .await
        .map_err(|_| "嵌入式登录状态任务失败".to_string())??;
        *self.user_id.lock().map_err(|_| "嵌入式用户状态锁已损坏")? = user_id;
        Ok(())
    }

    fn raw_session(&self) -> Result<Option<String>, String> {
        Ok(self
            .session
            .lock()
            .map_err(|_| "嵌入式登录状态锁已损坏")?
            .as_ref()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("imail_session="))
            .filter(|value| !value.is_empty())
            .map(str::to_owned))
    }

    fn current_user_id(&self) -> Result<Option<String>, String> {
        self.user_id
            .lock()
            .map_err(|_| "嵌入式用户状态锁已损坏".to_string())
            .map(|user_id| user_id.clone())
    }

    async fn event_context(&self) -> Result<(PathBuf, String, Vec<String>, i64), String> {
        let data_dir = self
            .data_dir
            .lock()
            .map_err(|_| "嵌入式服务状态锁已损坏")?
            .clone()
            .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
        let user_id = self
            .user_id
            .lock()
            .map_err(|_| "嵌入式用户状态锁已损坏")?
            .clone()
            .ok_or_else(|| "嵌入式服务尚未登录".to_string())?;
        let database = data_dir.join("imail.sqlite");
        let event_owner = user_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut auth = SqliteAuthStore::open_database(&database)
                .map_err(|error| format!("读取嵌入式登录状态失败：{error}"))?;
            let account_ids = AccountService::new(&mut auth)
                .list(&user_id)
                .map_err(|error| format!("读取嵌入式账户失败：{error}"))?
                .into_iter()
                .map(|account| account.id)
                .collect();
            let cursor = SyncRuntimeStore::open_database(&database)
                .and_then(|sync| sync.latest_event_id())
                .map_err(|error| format!("读取嵌入式事件游标失败：{error}"))?;
            Ok((data_dir, event_owner, account_ids, cursor))
        })
        .await
        .map_err(|_| "嵌入式事件初始化任务失败".to_string())?
    }

    fn replace_event_task(
        &self,
        task: Option<tauri::async_runtime::JoinHandle<()>>,
    ) -> Result<(), String> {
        let mut current = self
            .event_task
            .lock()
            .map_err(|_| "嵌入式事件状态锁已损坏")?;
        if let Some(previous) = current.take() {
            if let Some(host) = self.host.get() {
                host.sync_event_signal().notify();
            }
            previous.abort();
        }
        *current = task;
        Ok(())
    }

    pub fn shutdown(&self) -> Result<(), String> {
        self.replace_event_task(None)?;
        if let Some(mut adapter) = self
            .external_http
            .lock()
            .map_err(|_| "嵌入式 HTTP Adapter 状态锁已损坏")?
            .take()
        {
            if let Some(shutdown) = adapter.shutdown.take() {
                let _ = shutdown.send(());
            }
            adapter.task.abort();
        }
        if let Some(host) = self.host.get() {
            host.shutdown(Duration::from_secs(10))
                .map_err(|error| format!("关闭嵌入式 Rust 服务失败：{error}"))?;
        }
        Ok(())
    }
}

fn preferred_external_http_port(data_dir: &Path) -> Option<u16> {
    let path = data_dir.join(EXTERNAL_HTTP_PORT_FILE);
    let value = fs::read_to_string(&path).ok()?;
    match value.trim().parse::<u16>().ok().filter(|port| *port != 0) {
        Some(port) => Some(port),
        None => {
            log::warn!(target: "desktop", "[embedded.http.port_invalid] path={}", path.display());
            None
        }
    }
}

async fn bind_external_http_listener(data_dir: &Path) -> Result<tokio::net::TcpListener, String> {
    if let Some(port) = preferred_external_http_port(data_dir) {
        match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
            Ok(listener) => {
                log::info!(target: "desktop", "[embedded.http.port_reused] port={port}");
                return Ok(listener);
            }
            Err(error) => {
                log::warn!(target: "desktop", "[embedded.http.port_unavailable] port={port} error={error}");
            }
        }
    }
    tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| format!("启动嵌入式 HTTP Adapter 失败：{error}"))
}

fn persist_external_http_port(data_dir: &Path, port: u16) -> Result<(), String> {
    let path = data_dir.join(EXTERNAL_HTTP_PORT_FILE);
    let temporary = data_dir.join(format!(
        "{EXTERNAL_HTTP_PORT_FILE}.tmp-{}",
        std::process::id()
    ));
    fs::write(&temporary, port.to_string())
        .map_err(|error| format!("写入嵌入式 HTTP Adapter 端口失败：{error}"))?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|error| format!("更新嵌入式 HTTP Adapter 端口失败：{error}"))?;
    }
    fs::rename(&temporary, &path)
        .map_err(|error| format!("更新嵌入式 HTTP Adapter 端口失败：{error}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedHttpEndpoint {
    base_url: String,
}

#[tauri::command]
pub async fn desktop_start_external_http(
    app: AppHandle,
    state: State<'_, EmbeddedMailServiceState>,
) -> Result<EmbeddedHttpEndpoint, String> {
    let root = ensure_runtime_allowed(&app, &state).await?;
    state.initialize(root.join("data")).await?;
    state.start_external_http().await
}

#[derive(Clone, Serialize)]
struct EmbeddedSyncEvent {
    event: String,
    data: String,
}

fn prepare_empty_data_dir(data_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| format!("创建嵌入式数据目录失败：{error}"))?;
    let database = data_dir.join("imail.sqlite");
    if !database.exists() {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&database)
            .map_err(|error| format!("创建嵌入式数据库失败：{error}"))?;
    }
    migrate_database(&database).map_err(|error| format!("迁移嵌入式数据库失败：{error}"))?;
    let key = data_dir.join("master.key");
    if !key.exists() {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&key)
            .map_err(|error| format!("创建嵌入式主密钥失败：{error}"))?;
        file.write_all(MasterKey::generate_hex().as_bytes())
            .map_err(|error| format!("写入嵌入式主密钥失败：{error}"))?;
    }
    Ok(())
}

fn embedded_session_path(data_dir: &Path) -> Result<PathBuf, String> {
    data_dir
        .parent()
        .map(|root| root.join("embedded-session"))
        .ok_or_else(|| "嵌入式数据目录无效".to_string())
}

fn legacy_session_token(data_dir: &Path) -> Result<Option<String>, String> {
    let service_root = data_dir
        .parent()
        .ok_or_else(|| "嵌入式数据目录无效".to_string())?;
    if service_root.join("enabled").exists() {
        return Ok(None);
    }
    let daemon_path = service_root.join("daemon.json");
    let Ok(contents) = fs::read_to_string(daemon_path) else {
        return Ok(None);
    };
    let daemon: serde_json::Value =
        serde_json::from_str(&contents).map_err(|_| "旧本地服务配置无效".to_string())?;
    let configured_data = daemon
        .get("dataDir")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "旧本地服务配置缺少数据目录".to_string())?;
    let configured_data =
        fs::canonicalize(configured_data).map_err(|_| "旧本地服务数据目录无效".to_string())?;
    let actual_data =
        fs::canonicalize(data_dir).map_err(|_| "嵌入式数据目录无法验证".to_string())?;
    if configured_data != actual_data {
        return Err("旧本地服务配置的数据目录不匹配".into());
    }
    let host = daemon
        .get("host")
        .and_then(serde_json::Value::as_str)
        .filter(|host| matches!(*host, "127.0.0.1" | "localhost" | "::1"))
        .ok_or_else(|| "旧本地服务地址不是回环地址".to_string())?;
    let port = daemon
        .get("port")
        .and_then(serde_json::Value::as_u64)
        .filter(|port| (1..=65_535).contains(port))
        .ok_or_else(|| "旧本地服务端口无效".to_string())?;
    let app_root = service_root
        .parent()
        .ok_or_else(|| "旧登录状态目录无效".to_string())?;
    let service_base = if host == "::1" {
        format!("http://[::1]:{port}")
    } else {
        format!("http://{host}:{port}")
    };
    crate::http_bridge::persisted_session_token(&app_root.join("http-sessions"), &service_base)
}

fn write_private_session(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "嵌入式登录状态目录无效".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建嵌入式登录状态目录失败：{error}"))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("写入嵌入式登录状态失败：{error}"))?;
    file.write_all(contents)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("写入嵌入式登录状态失败：{error}"))?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("更新嵌入式登录状态失败：{error}"))?;
    }
    fs::rename(temporary, path).map_err(|error| format!("更新嵌入式登录状态失败：{error}"))
}

fn validate_request(input: &EmbeddedServiceRequest) -> Result<(), String> {
    if !input.path.starts_with("/api/")
        || input.path.starts_with("//")
        || input.path.contains("://")
        || input.path.contains('\r')
        || input.path.contains('\n')
    {
        return Err("嵌入式服务只接受相对 /api/ 路径".into());
    }
    if input
        .body
        .as_ref()
        .is_some_and(|body| body.len() > MAX_REQUEST_BYTES)
    {
        return Err("嵌入式服务请求体超过 25 MiB".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn desktop_mail_service_call(
    app: AppHandle,
    state: State<'_, EmbeddedMailServiceState>,
    call: EmbeddedDomainCall,
    request_id: Option<String>,
) -> Result<EmbeddedServiceResponse, String> {
    let cancellation = state.request_cancellations.begin(request_id.as_deref())?;
    let result = tokio::select! {
        result = async {
            let root = ensure_runtime_allowed(&app, &state).await?;
            state.initialize(root.join("data")).await?;
            state
                .direct_call(&call)
                .await?
                .ok_or_else(|| "嵌入式领域调用未实现".to_string())
        } => result,
        _ = wait_for_request_cancellation(cancellation) => Err("请求已取消".to_string()),
    };
    state.request_cancellations.finish(request_id.as_deref());
    result
}

#[tauri::command]
pub fn desktop_cancel_mail_service_call(
    state: State<'_, EmbeddedMailServiceState>,
    request_id: String,
) -> Result<bool, String> {
    state.request_cancellations.cancel(&request_id)
}

#[tauri::command]
pub async fn desktop_start_embedded_events(
    app: AppHandle,
    state: State<'_, EmbeddedMailServiceState>,
) -> Result<(), String> {
    ensure_runtime_allowed(&app, &state).await?;
    let (data_dir, user_id, account_ids, mut cursor) = state.event_context().await?;
    let connected = EmbeddedSyncEvent {
        event: "connected".into(),
        data: serde_json::json!({"connected": true}).to_string(),
    };
    let _ = app.emit("imail-sync-event", connected);
    let event_host = state
        .host
        .get()
        .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?;
    if let Ok(initial_status) = event_host.sync_status(user_id.clone()).await {
        let _ = app.emit(
            "imail-sync-event",
            EmbeddedSyncEvent {
                event: "sync.status".into(),
                data: initial_status.to_string(),
            },
        );
    }
    let event_app = app.clone();
    let event_signal = state
        .host
        .get()
        .ok_or_else(|| "嵌入式服务尚未初始化".to_string())?
        .sync_event_signal();
    let task = tauri::async_runtime::spawn(async move {
        let mut sequence = event_signal.sequence();
        let mut status_deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let wait = status_deadline.saturating_duration_since(Instant::now());
            let waiting_signal = event_signal.clone();
            let next_sequence =
                tokio::task::spawn_blocking(move || waiting_signal.wait_since(sequence, wait))
                    .await;
            let Ok(next_sequence) = next_sequence else {
                break;
            };
            sequence = next_sequence;
            let database = data_dir.join("imail.sqlite");
            let allowed = account_ids.clone();
            let batch = tokio::task::spawn_blocking(move || {
                let sync = SyncRuntimeStore::open_database(database)?;
                sync.events(cursor, 100)
            })
            .await;
            let Ok(Ok(events)) = batch else { break };
            for event in events {
                cursor = event.id;
                if allowed.contains(&event.account_id) {
                    let payload = EmbeddedSyncEvent {
                        event: event.event_type.clone(),
                        data: serde_json::to_string(&event).unwrap_or_else(|_| "{}".into()),
                    };
                    let _ = event_app.emit("imail-sync-event", payload);
                }
            }
            if Instant::now() >= status_deadline {
                let host = event_app.state::<EmbeddedMailServiceState>();
                if let Some(service) = host.host.get() {
                    if let Ok(response) = service.sync_status(user_id.clone()).await {
                        let _ = event_app.emit(
                            "imail-sync-event",
                            EmbeddedSyncEvent {
                                event: "sync.status".into(),
                                data: response.to_string(),
                            },
                        );
                    }
                }
                status_deadline = Instant::now() + Duration::from_secs(15);
            }
        }
    });
    state.replace_event_task(Some(task))
}

#[tauri::command]
pub fn desktop_stop_embedded_events(
    state: State<'_, EmbeddedMailServiceState>,
) -> Result<(), String> {
    state.replace_event_task(None)
}

#[tauri::command]
pub async fn desktop_read_embedded_binary(
    app: AppHandle,
    state: State<'_, EmbeddedMailServiceState>,
    path: String,
) -> Result<tauri::ipc::Response, String> {
    ensure_runtime_allowed(&app, &state).await?;
    state.read_binary(path).await.map(tauri::ipc::Response::new)
}

#[tauri::command]
pub async fn desktop_download_embedded(
    app: AppHandle,
    state: State<'_, EmbeddedMailServiceState>,
    path: String,
    target: PathBuf,
) -> Result<(), String> {
    ensure_runtime_allowed(&app, &state).await?;
    let bytes = state.read_binary(path).await?;
    fs::write(target, bytes).map_err(|error| format!("保存附件失败：{error}"))
}

async fn ensure_runtime_allowed(
    app: &AppHandle,
    state: &EmbeddedMailServiceState,
) -> Result<PathBuf, String> {
    let root = state
        .switch_root
        .get_or_try_init(|| crate::local_service::prepare_embedded_switch(app))
        .await?
        .clone();
    if root.join("enabled").exists() {
        return Err("旧本地服务仍处于启用状态，拒绝同时打开同一数据目录".into());
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use imail_core::{AccountRecord, AccountRepository};
    use imail_protocol::CURRENT_SCHEMA_VERSION;
    use imail_storage_sqlite::AppleHmeAddressRecord;
    use rusqlite::Connection;
    use sha2::{Digest, Sha256};

    #[test]
    fn rejects_absolute_and_oversized_webview_requests() {
        let request = |path: &str, body: Option<String>| EmbeddedServiceRequest {
            path: path.into(),
            method: "GET".into(),
            body,
        };
        assert!(validate_request(&request("https://example.test/api", None)).is_err());
        assert!(validate_request(&request("/api/system/info\r\nx: y", None)).is_err());
        assert!(validate_request(&request(
            "/api/system/info",
            Some("x".repeat(MAX_REQUEST_BYTES + 1)),
        ))
        .is_err());
        validate_request(&request("/api/system/info", None)).unwrap();
    }

    #[test]
    fn domain_calls_rebuild_routes_and_preserve_draft_idempotency() {
        let conversation = EmbeddedDomainCall::MessageConversation {
            message_id: "message/unsafe".into(),
        }
        .into_invocation();
        assert_eq!(
            conversation.request.path,
            "/api/messages/message%2Funsafe/conversation"
        );
        assert_eq!(conversation.request.method, "GET");
        let invocation = EmbeddedDomainCall::DraftCreate {
            draft_id: Some("draft-id".into()),
            input: serde_json::json!({"subject":"safe"}),
        }
        .into_invocation();
        assert_eq!(invocation.request.path, "/api/drafts");
        assert_eq!(invocation.request.method, "POST");
        assert_eq!(invocation.draft_id.as_deref(), Some("draft-id"));
        assert_eq!(
            invocation.request.body.as_deref(),
            Some(r#"{"subject":"safe"}"#)
        );

        let invocation = EmbeddedDomainCall::AccountDelete {
            account_id: "account/unsafe".into(),
        }
        .into_invocation();
        assert_eq!(invocation.request.path, "/api/accounts/account%2Funsafe");
        assert_eq!(invocation.request.method, "DELETE");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn serves_a_public_read_in_process_without_binding_a_listener() {
        let root =
            std::env::temp_dir().join(format!("imail-tauri-embedded-{}", uuid::Uuid::new_v4()));
        let data_dir = root.join("data");
        let state = EmbeddedMailServiceState::default();
        let response = state
            .request(
                data_dir,
                EmbeddedServiceRequest {
                    path: "/api/system/info".into(),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        let value: serde_json::Value = serde_json::from_str(&response.body).unwrap();
        assert_eq!(value["service"], "imail");
        assert_eq!(value["capabilities"]["gateway"], true);
        assert_eq!(value["capabilities"]["mcp"], true);
        assert_eq!(value["capabilities"]["syncWorker"], true);
        let auth = state
            .direct_call(&EmbeddedDomainCall::AuthStatus)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&auth.body).unwrap()["setupRequired"],
            true
        );
        state.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn migrates_an_existing_v6_database_before_restoring_the_session() {
        let root = std::env::temp_dir().join(format!(
            "imail-tauri-v6-session-migration-{}",
            uuid::Uuid::new_v4()
        ));
        let data_dir = root.join("data");
        prepare_empty_data_dir(&data_dir).unwrap();
        let database = data_dir.join("imail.sqlite");
        {
            let connection = Connection::open(&database).unwrap();
            connection
                .execute(
                    "INSERT INTO metadata(key,value) VALUES('migration_test_marker','preserved')",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE metadata SET value='6' WHERE key='schema_version'",
                    [],
                )
                .unwrap();
            connection
                .execute_batch("DROP TABLE mail_work_items; DROP TRIGGER messages_body_insert; DROP TRIGGER messages_body_delete; DROP TRIGGER messages_body_update; DROP TABLE message_body_fts; DROP TABLE smart_folders; DROP TABLE apple_hme_sessions; DROP TABLE outbox_items; ALTER TABLE messages DROP COLUMN mail_headers_json; ALTER TABLE drafts DROP COLUMN compose_json;")
                .unwrap();
        }
        write_private_session(
            &embedded_session_path(&data_dir).unwrap(),
            b"expired-session",
        )
        .unwrap();

        let state = EmbeddedMailServiceState::default();
        state.initialize(data_dir.clone()).await.unwrap();

        let connection = Connection::open(&database).unwrap();
        let schema_version: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let marker: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='migration_test_marker'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let hme_table_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='apple_hme_sessions')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(schema_version, CURRENT_SCHEMA_VERSION.to_string());
        assert_eq!(marker, "preserved");
        assert!(hme_table_exists);

        drop(connection);
        state.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn starts_one_loopback_http_adapter_for_mcp_and_gateway_and_reuses_its_port() {
        let root = std::env::temp_dir().join(format!(
            "imail-tauri-external-http-{}",
            uuid::Uuid::new_v4()
        ));
        let state = EmbeddedMailServiceState::default();
        state.initialize(root.join("data")).await.unwrap();

        let first = state.start_external_http().await.unwrap();
        let second = state.start_external_http().await.unwrap();
        assert_eq!(first.base_url, second.base_url);
        assert!(first.base_url.starts_with("http://127.0.0.1:"));
        assert!(!first.base_url.ends_with(":8787"));
        let first_port = first
            .base_url
            .rsplit_once(':')
            .unwrap()
            .1
            .parse::<u16>()
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("data").join(EXTERNAL_HTTP_PORT_FILE)).unwrap(),
            first_port.to_string()
        );

        let response = reqwest::get(format!("{}/api/system/info", first.base_url))
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let info: serde_json::Value =
            serde_json::from_str(&response.text().await.unwrap()).unwrap();
        assert_eq!(info["capabilities"]["gateway"], true);
        assert_eq!(info["capabilities"]["mcp"], true);
        let gateway = reqwest::get(format!("{}/gateway/v1/health", first.base_url))
            .await
            .unwrap();
        assert_eq!(gateway.status(), reqwest::StatusCode::OK);
        let mcp = reqwest::get(format!("{}/mcp", first.base_url))
            .await
            .unwrap();
        assert_eq!(mcp.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED);

        state.shutdown().unwrap();
        tokio::task::yield_now().await;

        let restarted = EmbeddedMailServiceState::default();
        restarted.initialize(root.join("data")).await.unwrap();
        let after_restart = restarted.start_external_http().await.unwrap();
        assert_eq!(after_restart.base_url, first.base_url);
        restarted.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn replaces_a_persisted_external_http_port_when_it_is_unavailable() {
        let root = std::env::temp_dir().join(format!(
            "imail-tauri-external-http-fallback-{}",
            uuid::Uuid::new_v4()
        ));
        let data_dir = root.join("data");
        prepare_empty_data_dir(&data_dir).unwrap();
        let occupied = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let occupied_port = occupied.local_addr().unwrap().port();
        fs::write(
            data_dir.join(EXTERNAL_HTTP_PORT_FILE),
            occupied_port.to_string(),
        )
        .unwrap();

        let state = EmbeddedMailServiceState::default();
        state.initialize(data_dir.clone()).await.unwrap();
        let endpoint = state.start_external_http().await.unwrap();
        let replacement_port = endpoint
            .base_url
            .rsplit_once(':')
            .unwrap()
            .1
            .parse::<u16>()
            .unwrap();
        assert_ne!(replacement_port, occupied_port);
        assert_eq!(
            fs::read_to_string(data_dir.join(EXTERNAL_HTTP_PORT_FILE)).unwrap(),
            replacement_port.to_string()
        );

        state.shutdown().unwrap();
        drop(occupied);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_authentication_matches_http_without_exposing_the_session() {
        let root = std::env::temp_dir().join(format!(
            "imail-tauri-auth-contract-{}",
            uuid::Uuid::new_v4()
        ));
        let direct_data = root.join("direct-host").join("data");
        let routed_data = root.join("routed-host").join("data");
        let direct = EmbeddedMailServiceState::default();
        let routed = EmbeddedMailServiceState::default();
        direct.initialize(direct_data.clone()).await.unwrap();
        routed.initialize(routed_data.clone()).await.unwrap();

        let registration = serde_json::json!({
            "login": " owner ",
            "displayName": " Owner ",
            "password": "correct horse battery staple"
        });
        let direct_registered = direct
            .direct_call(&EmbeddedDomainCall::AuthRegister {
                input: registration.clone(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_registered = routed
            .request(
                routed_data.clone(),
                EmbeddedServiceRequest {
                    path: "/api/auth/register".into(),
                    method: "POST".into(),
                    body: Some(registration.to_string()),
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_registered.status, routed_registered.status);
        for response in [&direct_registered, &routed_registered] {
            let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            assert_eq!(body["user"]["login"], "owner");
            assert_eq!(body["user"]["displayName"], "Owner");
            assert!(body["user"]["id"].as_str().is_some());
            assert!(!response.body.contains("imail_session"));
        }
        let direct_user_id = serde_json::from_str::<serde_json::Value>(&direct_registered.body)
            .unwrap()["user"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let routed_user_id = serde_json::from_str::<serde_json::Value>(&routed_registered.body)
            .unwrap()["user"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let direct_session_file = root.join("direct-host").join("embedded-session");
        let routed_session_file = root.join("routed-host").join("embedded-session");
        let direct_raw = fs::read_to_string(&direct_session_file).unwrap();
        let routed_raw = fs::read_to_string(&routed_session_file).unwrap();
        assert!(!direct_registered.body.contains(direct_raw.trim()));
        assert!(!routed_registered.body.contains(routed_raw.trim()));

        let direct_status = direct
            .direct_call(&EmbeddedDomainCall::AuthStatus)
            .await
            .unwrap()
            .unwrap();
        let routed_status = routed
            .request(
                routed_data.clone(),
                EmbeddedServiceRequest {
                    path: "/api/auth/status".into(),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_status.status, routed_status.status);
        for response in [&direct_status, &routed_status] {
            let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            assert_eq!(body["setupRequired"], false);
            assert_eq!(body["registrationOpen"], false);
            assert_eq!(body["user"]["login"], "owner");
        }

        let direct_logout = direct
            .direct_call(&EmbeddedDomainCall::AuthLogout)
            .await
            .unwrap()
            .unwrap();
        let routed_logout = routed
            .request(
                routed_data.clone(),
                EmbeddedServiceRequest {
                    path: "/api/auth/logout".into(),
                    method: "POST".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_logout.status, 204);
        assert_eq!(direct_logout.status, routed_logout.status);
        assert!(!direct_session_file.exists());
        assert!(!routed_session_file.exists());

        let invalid_login = serde_json::json!({
            "login": "owner",
            "password": "incorrect password"
        });
        let direct_invalid = direct
            .direct_call(&EmbeddedDomainCall::AuthLogin {
                input: invalid_login.clone(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_invalid = routed
            .request(
                routed_data.clone(),
                EmbeddedServiceRequest {
                    path: "/api/auth/login".into(),
                    method: "POST".into(),
                    body: Some(invalid_login.to_string()),
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_invalid.status, routed_invalid.status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct_invalid.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed_invalid.body).unwrap()
        );

        let login = serde_json::json!({
            "login": "owner",
            "password": "correct horse battery staple"
        });
        let direct_login = direct
            .direct_call(&EmbeddedDomainCall::AuthLogin {
                input: login.clone(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_login = routed
            .request(
                routed_data.clone(),
                EmbeddedServiceRequest {
                    path: "/api/auth/login".into(),
                    method: "POST".into(),
                    body: Some(login.to_string()),
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_login.status, routed_login.status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct_login.body).unwrap()["user"]["login"],
            "owner"
        );
        assert!(!direct_login.body.contains("imail_session"));

        for (data_dir, user_id) in [
            (&direct_data, direct_user_id.as_str()),
            (&routed_data, routed_user_id.as_str()),
        ] {
            let store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite")).unwrap();
            assert_eq!(
                store
                    .security_audit_events(user_id, 20)
                    .unwrap()
                    .iter()
                    .filter(|event| matches!(
                        event.event_type.as_str(),
                        "registration.succeeded" | "login.succeeded" | "logout"
                    ))
                    .count(),
                3
            );
        }
        direct.shutdown().unwrap();
        routed.shutdown().unwrap();
        let restarted = EmbeddedMailServiceState::default();
        restarted.initialize(direct_data).await.unwrap();
        let restarted_status = restarted
            .direct_call(&EmbeddedDomainCall::AuthStatus)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&restarted_status.body).unwrap()["user"]
                ["login"],
            "owner"
        );
        restarted.shutdown().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_account_metadata_and_removal_match_the_safe_http_contract() {
        let root = std::env::temp_dir().join(format!(
            "imail-tauri-account-contract-{}",
            uuid::Uuid::new_v4()
        ));
        let data_dir = root.join("data");
        let state = EmbeddedMailServiceState::default();
        state.initialize(data_dir.clone()).await.unwrap();
        state
            .direct_call(&EmbeddedDomainCall::AuthRegister {
                input: serde_json::json!({
                    "login": "owner",
                    "displayName": "Owner",
                    "password": "correct horse battery staple"
                }),
            })
            .await
            .unwrap()
            .unwrap();
        let user_id = state.current_user_id().unwrap().unwrap();
        let direct_invalid_create = state
            .direct_call(&EmbeddedDomainCall::AccountCreate {
                input: serde_json::json!({}),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_invalid_create = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: "/api/accounts".into(),
                    method: "POST".into(),
                    body: Some("{}".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_invalid_create.status, routed_invalid_create.status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct_invalid_create.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed_invalid_create.body).unwrap()
        );
        let first_id = uuid::Uuid::new_v4().to_string();
        let second_id = uuid::Uuid::new_v4().to_string();
        let icloud_id = uuid::Uuid::new_v4().to_string();
        let account = |id: &str, email: &str| AccountRecord {
            id: id.into(),
            owner_id: user_id.clone(),
            provider: "custom".into(),
            email: email.into(),
            display_name: "Original".into(),
            group: "个人".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: serde_json::json!({
                "imapHost":"imap.example.test",
                "imapPort":993,
                "imapSecure":true,
                "smtpHost":"smtp.example.test",
                "smtpPort":465,
                "smtpSecure":true,
                "password":"must-not-leak"
            }),
            proxy: Some(serde_json::json!({
                "protocol":"socks5",
                "host":"proxy.example.test",
                "port":1080,
                "username":"owner",
                "password":"must-not-leak"
            })),
            encrypted_secret: "encrypted-secret-must-not-leak".into(),
            auth_method: Some("app-password".into()),
            created_at: timestamp_now(),
            last_sync_at: None,
            status: "connected".into(),
            last_error: None,
            mailboxes: serde_json::json!([]),
        };
        let mut store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite")).unwrap();
        assert!(store
            .insert_account_if_email_available(&account(&first_id, "first@example.test"))
            .unwrap());
        assert!(store
            .insert_account_if_email_available(&account(&second_id, "second@example.test"))
            .unwrap());
        let mut icloud = account(&icloud_id, "owner@icloud.com");
        icloud.provider = "icloud".into();
        assert!(store.insert_account_if_email_available(&icloud).unwrap());
        store
            .replace_apple_hme_addresses(
                &user_id,
                &icloud_id,
                &[AppleHmeAddressRecord {
                    account_id: icloud_id.clone(),
                    user_id: user_id.clone(),
                    anonymous_id: "cached-hme-1".into(),
                    email: "quiet-path@icloud.com".into(),
                    label: "Shopping".into(),
                    note: String::new(),
                    forward_to_email: "owner@icloud.com".into(),
                    active: true,
                    origin: "icloud-web".into(),
                    created_at: Some("2026-08-18T00:00:00Z".into()),
                    updated_at: String::new(),
                }],
            )
            .unwrap();
        drop(store);

        let direct_list = state
            .direct_call(&EmbeddedDomainCall::AccountsList)
            .await
            .unwrap()
            .unwrap();
        let routed_list = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: "/api/accounts".into(),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_list.status, routed_list.status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct_list.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed_list.body).unwrap()
        );
        for forbidden in [
            "ownerId",
            "encryptedSecret",
            "encrypted-secret-must-not-leak",
            "must-not-leak",
        ] {
            assert!(!direct_list.body.contains(forbidden));
        }

        let direct_hme = state
            .direct_call(&EmbeddedDomainCall::AppleHmeList {
                account_id: icloud_id.clone(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_hme = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: format!("/api/accounts/{icloud_id}/apple-hme/addresses"),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_hme.status, 200, "{}", direct_hme.body);
        assert_eq!(direct_hme.body, routed_hme.body);
        let hme: serde_json::Value = serde_json::from_str(&direct_hme.body).unwrap();
        assert_eq!(hme["addressCount"], 1);
        assert_eq!(hme["addresses"][0]["email"], "quiet-path@icloud.com");
        assert!(hme["lastSyncedAt"].is_string());
        let mut store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite")).unwrap();
        assert!(store.delete_account(&user_id, &icloud_id).unwrap());
        drop(store);

        let patch = serde_json::json!({
            "displayName":" Renamed ",
            "group":" Work ",
            "groupIcon":"briefcase",
            "color":"#123abc"
        });
        let direct_update = state
            .direct_call(&EmbeddedDomainCall::AccountUpdate {
                account_id: first_id.clone(),
                input: patch.clone(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_update = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: format!("/api/accounts/{first_id}"),
                    method: "PATCH".into(),
                    body: Some(patch.to_string()),
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_update.status, routed_update.status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct_update.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed_update.body).unwrap()
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct_update.body).unwrap()["account"]
                ["displayName"],
            "Renamed"
        );

        let direct_invalid = state
            .direct_call(&EmbeddedDomainCall::AccountUpdate {
                account_id: first_id.clone(),
                input: serde_json::json!({}),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_invalid = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: format!("/api/accounts/{first_id}"),
                    method: "PATCH".into(),
                    body: Some("{}".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_invalid.status, routed_invalid.status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct_invalid.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed_invalid.body).unwrap()
        );

        let direct_delete = state
            .direct_call(&EmbeddedDomainCall::AccountDelete {
                account_id: first_id.clone(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_delete = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: format!("/api/accounts/{second_id}"),
                    method: "DELETE".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_delete.status, 204);
        assert_eq!(direct_delete.status, routed_delete.status);
        let store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite")).unwrap();
        assert_eq!(
            store
                .security_audit_details(&user_id, "account.removed", 10)
                .unwrap()
                .len(),
            2
        );
        drop(store);

        for (call, path, method, body) in [
            (
                EmbeddedDomainCall::AccountCredentialUpdate {
                    account_id: first_id.clone(),
                    input: serde_json::json!({"password":"replacement-secret"}),
                },
                format!("/api/accounts/{first_id}/credential"),
                "PUT",
                serde_json::json!({"password":"replacement-secret"}).to_string(),
            ),
            (
                EmbeddedDomainCall::AccountProxyUpdate {
                    account_id: first_id.clone(),
                    input: serde_json::json!({"enabled":false}),
                },
                format!("/api/accounts/{first_id}/proxy"),
                "PUT",
                serde_json::json!({"enabled":false}).to_string(),
            ),
        ] {
            let direct = state.direct_call(&call).await.unwrap().unwrap();
            let routed = state
                .request(
                    data_dir.clone(),
                    EmbeddedServiceRequest {
                        path: path.clone(),
                        method: method.into(),
                        body: Some(body),
                    },
                )
                .await
                .unwrap();
            assert_eq!(direct.status, routed.status, "{path}");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&direct.body).unwrap(),
                serde_json::from_str::<serde_json::Value>(&routed.body).unwrap(),
                "{path}"
            );
        }
        let direct_connection = state
            .direct_call(&EmbeddedDomainCall::AccountConnectionTest {
                account_id: first_id.clone(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_connection = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: format!("/api/accounts/{first_id}/connection-test"),
                    method: "POST".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_connection.status, routed_connection.status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct_connection.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed_connection.body).unwrap()
        );
        for (call, path, method, body) in [
            (
                EmbeddedDomainCall::OauthStart {
                    input: serde_json::json!({}),
                },
                "/api/oauth/start".to_string(),
                "POST",
                serde_json::json!({}),
            ),
            (
                EmbeddedDomainCall::OauthReconnect {
                    account_id: "missing-account".into(),
                },
                "/api/accounts/missing-account/oauth/reconnect".to_string(),
                "POST",
                serde_json::Value::Null,
            ),
            (
                EmbeddedDomainCall::OauthStatus {
                    input: serde_json::json!({"state":"unknown-state"}),
                },
                "/api/oauth/status".to_string(),
                "POST",
                serde_json::json!({"state":"unknown-state"}),
            ),
            (
                EmbeddedDomainCall::MessageUpdate {
                    message_id: "missing-message".into(),
                    unread: None,
                    flagged: None,
                    labels: None,
                    snoozed_until: None,
                },
                "/api/messages/missing-message".to_string(),
                "PATCH",
                serde_json::json!({
                    "unread":null,
                    "flagged":null,
                    "labels":null,
                    "snoozedUntil":null
                }),
            ),
            (
                EmbeddedDomainCall::MessageMove {
                    message_id: "missing-message".into(),
                    destination: "archive".into(),
                },
                "/api/messages/missing-message/move".to_string(),
                "POST",
                serde_json::json!({"destination":"archive"}),
            ),
            (
                EmbeddedDomainCall::MessageSend {
                    input: serde_json::json!({}),
                },
                "/api/send".to_string(),
                "POST",
                serde_json::json!({}),
            ),
            (
                EmbeddedDomainCall::SyncAll,
                "/api/sync".to_string(),
                "POST",
                serde_json::Value::Null,
            ),
            (
                EmbeddedDomainCall::SyncAccount {
                    account_id: "missing-account".into(),
                },
                "/api/accounts/missing-account/sync".to_string(),
                "POST",
                serde_json::Value::Null,
            ),
            (
                EmbeddedDomainCall::SyncAccountMailbox {
                    account_id: "missing-account".into(),
                    mailbox: "".into(),
                },
                "/api/accounts/missing-account/mailboxes/sync".to_string(),
                "POST",
                serde_json::json!({"mailbox":""}),
            ),
            (
                EmbeddedDomainCall::SyncMailboxRole {
                    role: "invalid".into(),
                },
                "/api/mailboxes/invalid/sync".to_string(),
                "POST",
                serde_json::Value::Null,
            ),
            (
                EmbeddedDomainCall::AuthorizationExportPrepare {
                    input: serde_json::json!({
                        "currentPassword":"x",
                        "exportPassword":"short"
                    }),
                },
                "/api/security/mail-authorization-exports".to_string(),
                "POST",
                serde_json::json!({
                    "currentPassword":"x",
                    "exportPassword":"short"
                }),
            ),
            (
                EmbeddedDomainCall::UserDataClear {
                    input: serde_json::json!({
                        "currentPassword":"correct horse battery staple",
                        "confirmation":"wrong"
                    }),
                },
                "/api/security/clear-user-data".to_string(),
                "POST",
                serde_json::json!({
                    "currentPassword":"correct horse battery staple",
                    "confirmation":"wrong"
                }),
            ),
        ] {
            let direct = state.direct_call(&call).await.unwrap().unwrap();
            let routed = state
                .request(
                    data_dir.clone(),
                    EmbeddedServiceRequest {
                        path: path.clone(),
                        method: method.into(),
                        body: (body != serde_json::Value::Null).then(|| body.to_string()),
                    },
                )
                .await
                .unwrap();
            assert_eq!(direct.status, routed.status, "{path}");
            if direct.status == 200 {
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&direct.body).unwrap(),
                    serde_json::from_str::<serde_json::Value>(&routed.body).unwrap(),
                    "{path}"
                );
            }
        }
        let prepared = state
            .direct_call(&EmbeddedDomainCall::AuthorizationExportPrepare {
                input: serde_json::json!({
                    "currentPassword":"correct horse battery staple",
                    "exportPassword":"portable-password-123"
                }),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(prepared.status, 200);
        let prepared: serde_json::Value = serde_json::from_str(&prepared.body).unwrap();
        assert_eq!(prepared["accountCount"], 0);
        let download_path = prepared["downloadPath"].as_str().unwrap().to_string();
        let export = state.read_binary(download_path.clone()).await.unwrap();
        assert!(serde_json::from_slice::<serde_json::Value>(&export).is_ok());
        assert!(state.read_binary(download_path).await.is_err());
        assert!(state
            .read_binary("/api/messages/missing-message/attachments/0".into())
            .await
            .is_err());
        assert!(state
            .read_binary("/api/contacts/logo?address=missing%40example.test".into())
            .await
            .is_err());

        let cleared = state
            .direct_call(&EmbeddedDomainCall::UserDataClear {
                input: serde_json::json!({
                    "currentPassword":"correct horse battery staple",
                    "confirmation":"清除我的邮箱数据"
                }),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cleared.status, 204);
        let store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite")).unwrap();
        let audit = store.security_audit_events(&user_id, 20).unwrap();
        assert!(audit
            .iter()
            .any(|event| event.event_type == "privacy.mail-authorization-export.prepared"));
        assert!(audit
            .iter()
            .any(|event| event.event_type == "privacy.mail-authorization-export.downloaded"));
        assert!(audit
            .iter()
            .any(|event| event.event_type == "privacy.user-data-cleared"));
        drop(store);
        state.shutdown().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn keeps_the_local_session_inside_the_rust_host() {
        let root =
            std::env::temp_dir().join(format!("imail-tauri-session-{}", uuid::Uuid::new_v4()));
        let data_dir = root.join("data");
        let state = EmbeddedMailServiceState::default();
        let register = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: "/api/auth/register".into(),
                    method: "POST".into(),
                    body: Some(r#"{"login":"owner","displayName":"Owner","password":"correct horse battery staple"}"#.into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(register.status, 201);
        assert!(!register.body.contains("imail_session"));
        let status = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: "/api/auth/status".into(),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(status.status, 200);
        let value: serde_json::Value = serde_json::from_str(&status.body).unwrap();
        assert_eq!(value["user"]["login"], "owner");
        for (call, path) in [
            (EmbeddedDomainCall::AccountsList, "/api/accounts"),
            (EmbeddedDomainCall::DraftsList, "/api/drafts"),
            (EmbeddedDomainCall::MessageStats, "/api/message-stats"),
            (EmbeddedDomainCall::LabelsList, "/api/labels"),
            (EmbeddedDomainCall::NotificationsList, "/api/notifications"),
            (EmbeddedDomainCall::MailRulesList, "/api/mail-rules"),
            (EmbeddedDomainCall::MailRuleRuns, "/api/mail-rule-runs"),
            (EmbeddedDomainCall::PreferencesGet, "/api/preferences"),
            (
                EmbeddedDomainCall::DeveloperTokensList,
                "/api/developer-tokens",
            ),
            (
                EmbeddedDomainCall::ExternalAccessGet,
                "/api/external-access",
            ),
        ] {
            let direct = state.direct_call(&call).await.unwrap().unwrap();
            let routed = state
                .request(
                    data_dir.clone(),
                    EmbeddedServiceRequest {
                        path: path.into(),
                        method: "GET".into(),
                        body: None,
                    },
                )
                .await
                .unwrap();
            assert_eq!(direct.status, routed.status, "{path}");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&direct.body).unwrap(),
                serde_json::from_str::<serde_json::Value>(&routed.body).unwrap(),
                "{path}"
            );
        }
        let message_query = BTreeMap::from([
            ("limit".to_string(), "60".to_string()),
            ("offset".to_string(), "0".to_string()),
        ]);
        let direct = state
            .direct_call(&EmbeddedDomainCall::MessagesList {
                query: message_query,
            })
            .await
            .unwrap()
            .unwrap();
        let routed = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: "/api/messages?limit=60&offset=0".into(),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct.status, routed.status, "/api/messages");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed.body).unwrap(),
            "/api/messages"
        );

        let direct = state
            .direct_call(&EmbeddedDomainCall::ContactsList)
            .await
            .unwrap()
            .unwrap();
        let routed = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: "/api/contacts".into(),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct.status, routed.status, "/api/contacts");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed.body).unwrap(),
            "/api/contacts"
        );

        let missing_message_id = "missing-message";
        let conversation = state
            .direct_call(&EmbeddedDomainCall::MessageConversation {
                message_id: missing_message_id.into(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_conversation = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: format!("/api/messages/{missing_message_id}/conversation"),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(conversation.status, 404);
        assert_eq!(conversation.status, routed_conversation.status);
        assert_eq!(conversation.body, routed_conversation.body);
        let direct = state
            .direct_call(&EmbeddedDomainCall::MessageDetail {
                message_id: missing_message_id.into(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: format!("/api/messages/{missing_message_id}"),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct.status, routed.status, "/api/messages/:id");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed.body).unwrap(),
            "/api/messages/:id"
        );

        for (call, path, body) in [
            (
                EmbeddedDomainCall::PreferencesUpdate {
                    input: serde_json::json!({"markReadOnOpen":false}),
                },
                "/api/preferences",
                r#"{"markReadOnOpen":false}"#,
            ),
            (
                EmbeddedDomainCall::ExternalAccessUpdate {
                    input: serde_json::json!({"gatewayEnabled":true}),
                },
                "/api/external-access",
                r#"{"gatewayEnabled":true}"#,
            ),
        ] {
            let direct = state.direct_call(&call).await.unwrap().unwrap();
            let routed = state
                .request(
                    data_dir.clone(),
                    EmbeddedServiceRequest {
                        path: path.into(),
                        method: "PATCH".into(),
                        body: Some(body.into()),
                    },
                )
                .await
                .unwrap();
            assert_eq!(direct.status, routed.status, "{path}");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&direct.body).unwrap(),
                serde_json::from_str::<serde_json::Value>(&routed.body).unwrap(),
                "{path}"
            );
        }

        for (call, path, method, body) in [
            (
                EmbeddedDomainCall::PreferencesUpdate {
                    input: serde_json::json!({}),
                },
                "/api/preferences",
                "PATCH",
                "{}",
            ),
            (
                EmbeddedDomainCall::ExternalAccessUpdate {
                    input: serde_json::json!({}),
                },
                "/api/external-access",
                "PATCH",
                "{}",
            ),
        ] {
            let direct = state.direct_call(&call).await.unwrap().unwrap();
            let routed = state
                .request(
                    data_dir.clone(),
                    EmbeddedServiceRequest {
                        path: path.into(),
                        method: method.into(),
                        body: Some(body.into()),
                    },
                )
                .await
                .unwrap();
            assert_eq!(direct.status, routed.status, "{path} invalid");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&direct.body).unwrap(),
                serde_json::from_str::<serde_json::Value>(&routed.body).unwrap(),
                "{path} invalid"
            );
        }

        let missing_account_id = uuid::Uuid::new_v4().to_string();
        let draft_input = serde_json::json!({
            "accountId": missing_account_id,
            "to": ["receiver@example.test"],
            "subject": "contract",
            "text": "body"
        });
        let draft_body = draft_input.to_string();
        for (call, path, method) in [
            (
                EmbeddedDomainCall::DraftCreate {
                    draft_id: Some(uuid::Uuid::new_v4().to_string()),
                    input: draft_input.clone(),
                },
                "/api/drafts".to_string(),
                "POST",
            ),
            (
                EmbeddedDomainCall::DraftUpdate {
                    draft_id: "missing-draft".into(),
                    input: draft_input.clone(),
                },
                "/api/drafts/missing-draft".to_string(),
                "PUT",
            ),
        ] {
            let direct = state.direct_call(&call).await.unwrap().unwrap();
            let routed = state
                .request(
                    data_dir.clone(),
                    EmbeddedServiceRequest {
                        path: path.clone(),
                        method: method.into(),
                        body: Some(draft_body.clone()),
                    },
                )
                .await
                .unwrap();
            assert_eq!(direct.status, routed.status, "{path}");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&direct.body).unwrap(),
                serde_json::from_str::<serde_json::Value>(&routed.body).unwrap(),
                "{path}"
            );
        }

        let direct = state
            .direct_call(&EmbeddedDomainCall::DraftDelete {
                draft_id: "missing-draft".into(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: "/api/drafts/missing-draft".into(),
                    method: "DELETE".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct.status, routed.status, "/api/drafts/:id delete");
        assert_eq!(direct.body, routed.body, "/api/drafts/:id delete");

        let invalid_token = state
            .direct_call(&EmbeddedDomainCall::DeveloperTokenCreate {
                input: serde_json::json!({}),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_invalid_token = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: "/api/developer-tokens".into(),
                    method: "POST".into(),
                    body: Some("{}".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(invalid_token.status, routed_invalid_token.status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&invalid_token.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed_invalid_token.body).unwrap()
        );

        let token_input = serde_json::json!({
            "name": "MCP contract",
            "scopes": ["messages:read", "mcp:full"],
            "mailboxes": [],
            "ttlSeconds": 3600
        });
        let direct_created = state
            .direct_call(&EmbeddedDomainCall::DeveloperTokenCreate {
                input: token_input.clone(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_created = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: "/api/developer-tokens".into(),
                    method: "POST".into(),
                    body: Some(token_input.to_string()),
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_created.status, 201);
        assert_eq!(routed_created.status, 201);
        let direct_created: serde_json::Value = serde_json::from_str(&direct_created.body).unwrap();
        let routed_created: serde_json::Value = serde_json::from_str(&routed_created.body).unwrap();
        for created in [&direct_created, &routed_created] {
            let raw = created["token"].as_str().unwrap();
            assert!(raw.starts_with("imail_mcp_"));
            assert_eq!(created["detail"]["prefix"], &raw[..12]);
            assert_eq!(created["detail"]["scopes"], serde_json::json!(["mcp:full"]));
            assert_eq!(created["detail"]["mailboxes"], serde_json::json!([]));
            assert!(created["detail"].get("ownerId").is_none());
            assert!(created["detail"].get("accountIds").is_none());
            assert!(created["detail"].get("tokenHash").is_none());
        }
        let direct_raw = direct_created["token"].as_str().unwrap();
        let routed_raw = routed_created["token"].as_str().unwrap();
        let listed = state
            .direct_call(&EmbeddedDomainCall::DeveloperTokensList)
            .await
            .unwrap()
            .unwrap();
        let routed_listed = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: "/api/developer-tokens".into(),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(listed.status, routed_listed.status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&listed.body).unwrap(),
            serde_json::from_str::<serde_json::Value>(&routed_listed.body).unwrap()
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&listed.body).unwrap()["tokens"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(!listed.body.contains(direct_raw));
        assert!(!listed.body.contains(routed_raw));

        let direct_token_id = direct_created["detail"]["id"].as_str().unwrap();
        let routed_token_id = routed_created["detail"]["id"].as_str().unwrap();
        let direct_deleted = state
            .direct_call(&EmbeddedDomainCall::DeveloperTokenDelete {
                token_id: direct_token_id.into(),
            })
            .await
            .unwrap()
            .unwrap();
        let routed_deleted = state
            .request(
                data_dir.clone(),
                EmbeddedServiceRequest {
                    path: format!("/api/developer-tokens/{routed_token_id}"),
                    method: "DELETE".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direct_deleted.status, 204);
        assert_eq!(routed_deleted.status, 204);
        let user_id = state.current_user_id().unwrap().unwrap();
        let store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite")).unwrap();
        let created_audits = store
            .security_audit_details(&user_id, "developer-token.created", 10)
            .unwrap();
        let revoked_audits = store
            .security_audit_details(&user_id, "developer-token.revoked", 10)
            .unwrap();
        assert_eq!(created_audits.len(), 2);
        assert_eq!(revoked_audits.len(), 2);
        assert!(created_audits.iter().all(|detail| {
            detail.get("scopes").map(String::as_str) == Some("mcp:full")
                && detail.get("mailboxCount").map(String::as_str) == Some("0")
        }));
        assert!(state.read_binary("/api/auth/status".into()).await.is_err());
        let (event_data_dir, event_owner, account_ids, cursor) =
            state.event_context().await.unwrap();
        assert_eq!(event_data_dir, data_dir);
        assert_eq!(event_owner, state.current_user_id().unwrap().unwrap());
        assert!(account_ids.is_empty());
        assert_eq!(cursor, 0);
        state.shutdown().unwrap();

        let restarted = EmbeddedMailServiceState::default();
        let status = restarted
            .request(
                root.join("data"),
                EmbeddedServiceRequest {
                    path: "/api/auth/status".into(),
                    method: "GET".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&status.body).unwrap()["user"]["login"],
            "owner"
        );
        restarted
            .request(
                root.join("data"),
                EmbeddedServiceRequest {
                    path: "/api/auth/logout".into(),
                    method: "POST".into(),
                    body: None,
                },
            )
            .await
            .unwrap();
        assert!(!root.join("embedded-session").exists());
        restarted.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn imports_a_valid_legacy_http_session_once_without_exposing_it() {
        let app_root = std::env::temp_dir().join(format!(
            "imail-tauri-session-import-{}",
            uuid::Uuid::new_v4()
        ));
        let service_root = app_root.join("local-service");
        let data_dir = service_root.join("data");
        let original = EmbeddedMailServiceState::default();
        original.initialize(data_dir.clone()).await.unwrap();
        let registered = original
            .direct_call(&EmbeddedDomainCall::AuthRegister {
                input: serde_json::json!({
                    "login":"owner",
                    "displayName":"Owner",
                    "password":"correct horse battery staple"
                }),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(registered.status, 201);
        let raw_session = original.raw_session().unwrap().unwrap();
        original.shutdown().unwrap();
        fs::remove_file(service_root.join("embedded-session")).unwrap();

        let service_base = "http://127.0.0.1:8787";
        fs::write(
            service_root.join("daemon.json"),
            serde_json::json!({
                "dataDir":fs::canonicalize(&data_dir).unwrap(),
                "host":"127.0.0.1",
                "port":8787
            })
            .to_string(),
        )
        .unwrap();
        let cookie_root = app_root.join("http-sessions");
        fs::create_dir_all(&cookie_root).unwrap();
        let mut cookies = reqwest_cookie_store::CookieStore::default();
        cookies
            .parse(
                &format!(
                    "imail_session={raw_session}; Path=/; Max-Age=2592000; HttpOnly; SameSite=Lax"
                ),
                &url::Url::parse(service_base).unwrap(),
            )
            .unwrap();
        let mut cookie_json = Vec::new();
        cookie_store::serde::json::save(&cookies, &mut cookie_json).unwrap();
        let cookie_name = format!("{:x}.json", Sha256::digest(service_base.as_bytes()));
        fs::write(cookie_root.join(cookie_name), cookie_json).unwrap();

        let imported = EmbeddedMailServiceState::default();
        imported.initialize(data_dir).await.unwrap();
        let status = imported
            .direct_call(&EmbeddedDomainCall::AuthStatus)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.status, 200);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&status.body).unwrap()["user"]["login"],
            "owner"
        );
        assert!(service_root.join("embedded-session").is_file());
        assert!(!status.body.contains(&raw_session));
        imported.shutdown().unwrap();
        fs::remove_dir_all(app_root).unwrap();
    }
}
