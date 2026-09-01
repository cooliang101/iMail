use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod composition;
mod translation;

pub use composition::*;
pub use translation::*;

pub const CURRENT_SCHEMA_VERSION: u32 = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceErrorCode {
    InvalidInput,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    StorageUnavailable,
    UnsupportedSchema,
    CorruptData,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceErrorView {
    pub code: ServiceErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUserView {
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitDecision {
    pub allowed: bool,
    pub retry_after: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuditEventReadModel {
    pub id: String,
    pub event_type: String,
    pub actor_hash: String,
    pub detail: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeId {
    MintFresh,
    Tech,
    BusinessBlue,
    SoftNeubrutalism,
    ConstructivistRed,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupView {
    Inbox,
    Starred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageView {
    Source,
    Rendered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationKinds {
    pub unread: bool,
    pub snooze: bool,
    pub error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBindings {
    pub focus_search: String,
    pub compose: String,
    pub sync: String,
    pub next_message: String,
    pub previous_message: String,
    pub reply: String,
    pub forward: String,
    pub toggle_star: String,
    pub mark_unread: String,
    pub archive: String,
    pub delete: String,
    pub open_shortcut_settings: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    #[serde(default, skip_serializing_if = "CompositionPreferences::is_empty")]
    pub composition: CompositionPreferences,
    pub language: AppLanguage,
    pub theme: ThemeId,
    pub custom_theme: CustomTheme,
    pub startup_view: StartupView,
    pub mark_read_on_open: bool,
    pub default_message_view: MessageView,
    pub notification_kinds: NotificationKinds,
    pub shortcut_bindings: ShortcutBindings,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            language: AppLanguage::ZhCn,
            composition: CompositionPreferences::default(),
            theme: ThemeId::MintFresh,
            custom_theme: CustomTheme::default(),
            startup_view: StartupView::Inbox,
            mark_read_on_open: true,
            default_message_view: MessageView::Source,
            notification_kinds: NotificationKinds {
                unread: true,
                snooze: true,
                error: true,
            },
            shortcut_bindings: ShortcutBindings {
                focus_search: "Mod+K".into(),
                compose: "C".into(),
                sync: "Mod+Shift+R".into(),
                next_message: "ArrowRight".into(),
                previous_message: "ArrowLeft".into(),
                reply: "R".into(),
                forward: "F".into(),
                toggle_star: "S".into(),
                mark_unread: "U".into(),
                archive: "A".into(),
                delete: "Delete".into(),
                open_shortcut_settings: "Mod+/".into(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppLanguage {
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en-US")]
    EnUs,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationKindsPatch {
    pub unread: Option<bool>,
    pub snooze: Option<bool>,
    pub error: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBindingsPatch {
    pub focus_search: Option<String>,
    pub compose: Option<String>,
    pub sync: Option<String>,
    pub next_message: Option<String>,
    pub previous_message: Option<String>,
    pub reply: Option<String>,
    pub forward: Option<String>,
    pub toggle_star: Option<String>,
    pub mark_unread: Option<String>,
    pub archive: Option<String>,
    pub delete: Option<String>,
    pub open_shortcut_settings: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferencesPatch {
    pub composition: Option<CompositionPreferences>,
    pub language: Option<AppLanguage>,
    pub theme: Option<ThemeId>,
    pub custom_theme: Option<CustomTheme>,
    pub startup_view: Option<StartupView>,
    pub mark_read_on_open: Option<bool>,
    pub default_message_view: Option<MessageView>,
    pub notification_kinds: Option<NotificationKindsPatch>,
    pub shortcut_bindings: Option<ShortcutBindingsPatch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftInput {
    #[serde(default, flatten)]
    pub envelope: ComposeEnvelope,
    pub account_id: String,
    pub to: Value,
    pub cc: Value,
    pub subject: String,
    pub text: String,
    pub html: String,
    pub attachments: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceIconId {
    Folder,
    Briefcase,
    Building,
    Home,
    Users,
    Code,
    Heart,
    Star,
}

impl WorkspaceIconId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Briefcase => "briefcase",
            Self::Building => "building",
            Self::Home => "home",
            Self::Users => "users",
            Self::Code => "code",
            Self::Heart => "heart",
            Self::Star => "star",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountMetadataPatch {
    pub display_name: Option<String>,
    pub group: Option<String>,
    pub group_icon: Option<WorkspaceIconId>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyProtocol {
    Http,
    Https,
    Socks5,
}

impl ProxyProtocol {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
            Self::Socks5 => "socks5",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum AccountProxyUpdate {
    Disabled,
    CopyFrom {
        source_account_id: String,
    },
    Explicit {
        protocol: ProxyProtocol,
        host: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearMailDataResult {
    pub account_count: u64,
}

pub const MAIL_AUTHORIZATION_EXPORT_FORMAT: &str = "imail-mail-authorizations";
pub const MAIL_AUTHORIZATION_EXPORT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailSettingsExport {
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_secure: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_secure: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailProxyExport {
    pub protocol: String,
    pub host: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAuthorizationSecretExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_password: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAuthorizationAccountExport {
    pub provider: String,
    pub email: String,
    pub display_name: String,
    pub group: String,
    pub group_icon: String,
    pub color: String,
    pub auth_method: String,
    pub settings: MailSettingsExport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<MailProxyExport>,
    pub authorization: MailAuthorizationSecretExport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAuthorizationExportPayload {
    pub format: String,
    pub format_version: u32,
    pub exported_at: String,
    pub accounts: Vec<MailAuthorizationAccountExport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAuthorizationExportKdf {
    pub algorithm: String,
    pub salt: String,
    pub cost: u32,
    pub block_size: u32,
    pub parallelization: u32,
    pub key_length: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAuthorizationExportCipher {
    pub algorithm: String,
    pub iv: String,
    pub auth_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAuthorizationExportEnvelope {
    pub format: String,
    pub format_version: u32,
    pub kdf: MailAuthorizationExportKdf,
    pub cipher: MailAuthorizationExportCipher,
    pub ciphertext: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAuthorizationExportArtifact {
    pub filename: String,
    pub account_count: usize,
    pub envelope: MailAuthorizationExportEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataBackupResult {
    pub backup_root: String,
    pub schema_version: u32,
    pub file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRestoreResult {
    pub restore_root: String,
    pub schema_version: u32,
    pub integrity_manifest_verified: bool,
    pub master_key_included: bool,
    pub instance_id_included: bool,
    pub sender_logos_included: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAddressView {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedAttachmentView {
    pub filename: String,
    pub content_type: String,
    pub size: usize,
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedMailView {
    #[serde(default, flatten)]
    pub headers: MailHeaders,
    pub message_id: Option<String>,
    pub from: MailAddressView,
    pub to: Vec<MailAddressView>,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub date: Option<String>,
    pub preview: String,
    pub attachments: Vec<ParsedAttachmentView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageMoveDestination {
    Archive,
    Trash,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMessageFlagPatch {
    pub unread: Option<bool>,
    pub flagged: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMessageMoveResult {
    pub mailbox: String,
    pub uid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAttachmentInput {
    pub filename: String,
    pub content_type: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageInput {
    #[serde(default, flatten)]
    pub envelope: ComposeEnvelope,
    pub account_id: String,
    pub to: Vec<String>,
    pub cc: Option<Vec<String>>,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub attachments: Option<Vec<SendAttachmentInput>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResult {
    pub message_id: String,
    pub accepted: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutboxStatus {
    Scheduled,
    Sending,
    Sent,
    Failed,
    NeedsReview,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxItemReadModel {
    pub id: String,
    pub account_id: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub scheduled_at: String,
    pub status: OutboxStatus,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationKind {
    Error,
    Snooze,
    Unread,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationView {
    pub id: String,
    pub kind: NotificationKind,
    pub title: String,
    pub detail: String,
    pub date: String,
    pub message_id: Option<String>,
    pub account_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeRadius {
    Compact,
    Balanced,
    Rounded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeShadow {
    None,
    Soft,
    Offset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeTypography {
    System,
    Technical,
    Rounded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomTheme {
    pub name: String,
    pub canvas: String,
    pub surface: String,
    pub surface_subtle: String,
    pub rail: String,
    pub text: String,
    pub text_secondary: String,
    pub border: String,
    pub accent: String,
    pub accent_subtle: String,
    pub radius: ThemeRadius,
    pub shadow: ThemeShadow,
    pub typography: ThemeTypography,
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self {
            name: "我的主题".into(),
            canvas: "#e9edf4".into(),
            surface: "#fbfcff".into(),
            surface_subtle: "#f2f5fa".into(),
            rail: "#20283a".into(),
            text: "#202536".into(),
            text_secondary: "#677086".into(),
            border: "#d4dae6".into(),
            accent: "#d06f52".into(),
            accent_subtle: "#f8e8e2".into(),
            radius: ThemeRadius::Balanced,
            shadow: ThemeShadow::Soft,
            typography: ThemeTypography::System,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInventory {
    pub schema_version: u32,
    pub quick_check: bool,
    pub foreign_keys_verified: bool,
    pub table_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialCompatibilitySummary {
    pub account_count: u64,
    pub decrypted_count: u64,
    pub field_counts: BTreeMap<String, u64>,
    pub translation_credential_count: u64,
    pub translation_decrypted_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountReadModel {
    pub id: String,
    pub owner_id: String,
    pub provider: String,
    pub email: String,
    pub display_name: String,
    pub group: String,
    pub group_icon: String,
    pub color: String,
    pub settings: Value,
    pub proxy: Option<Value>,
    pub auth_method: Option<String>,
    pub created_at: String,
    pub last_sync_at: Option<String>,
    pub status: String,
    pub last_error: Option<String>,
    pub mailboxes: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReadModel {
    #[serde(default, flatten)]
    pub headers: MailHeaders,
    pub id: String,
    pub account_id: String,
    pub mailbox: String,
    pub mailbox_role: String,
    pub uid: i64,
    pub message_id: Option<String>,
    pub from: Value,
    pub to: Value,
    pub subject: String,
    pub preview: String,
    pub text: String,
    pub html: Option<String>,
    pub date: String,
    pub unread: bool,
    pub flagged: bool,
    pub has_attachments: bool,
    pub attachments: Value,
    pub labels: Value,
    pub snoozed_until: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftReadModel {
    #[serde(default, flatten)]
    pub envelope: ComposeEnvelope,
    pub id: String,
    pub account_id: String,
    pub to: Value,
    pub cc: Value,
    pub subject: String,
    pub text: String,
    pub html: String,
    pub attachments: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactReadModel {
    pub owner_id: String,
    pub address: String,
    pub name: String,
    pub message_count: i64,
    pub last_contact_at: String,
    pub logo_key: Option<String>,
    pub logo_content_type: Option<String>,
    pub logo_source_url: Option<String>,
    pub logo_fetched_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperTokenReadModel {
    pub id: String,
    pub owner_id: String,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub account_ids: Vec<String>,
    pub created_at: String,
    pub expires_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPolicyReadModel {
    pub account_id: String,
    pub enabled: bool,
    pub folder_mode: String,
    pub selected_mailboxes: Value,
    pub notify_on_error: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSyncStateReadModel {
    pub account_id: String,
    pub mailbox: String,
    pub mailbox_role: String,
    pub uid_validity: Option<String>,
    pub last_seen_uid: i64,
    pub highest_modseq: Option<String>,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub next_sync_at: Option<String>,
    pub consecutive_failures: i64,
    pub connection_status: String,
    pub sync_state: String,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncJobReadModel {
    pub id: String,
    pub account_id: String,
    pub mailbox: Option<String>,
    pub mailbox_role: String,
    pub reason: String,
    pub status: String,
    pub priority: i64,
    pub not_before: String,
    pub locked_by: Option<String>,
    pub locked_until: Option<String>,
    pub attempts: i64,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub synced_count: Option<i64>,
    pub new_count: Option<i64>,
    pub updated_count: Option<i64>,
    pub deleted_count: Option<i64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub rerun_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEventReadModel {
    pub id: i64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub account_id: String,
    pub job_id: Option<String>,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncWorkerReadModel {
    pub worker_id: String,
    pub process_id: i64,
    pub host_name: String,
    pub started_at: String,
    pub heartbeat_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncWorkerHealthReadModel {
    pub workers: Vec<SyncWorkerReadModel>,
    pub queued_jobs: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_queued_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadOnlySnapshot {
    pub accounts: Vec<AccountReadModel>,
    pub messages: Vec<MessageReadModel>,
    pub drafts: Vec<DraftReadModel>,
    pub contacts: Vec<ContactReadModel>,
    pub developer_tokens: Vec<DeveloperTokenReadModel>,
    pub sync_policies: Vec<SyncPolicyReadModel>,
    pub mailbox_sync_states: Vec<MailboxSyncStateReadModel>,
    pub sync_jobs: Vec<SyncJobReadModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadModelCounts {
    pub accounts: usize,
    pub messages: usize,
    pub drafts: usize,
    pub contacts: usize,
    pub developer_tokens: usize,
    pub sync_policies: usize,
    pub mailbox_sync_states: usize,
    pub sync_jobs: usize,
}

impl ReadOnlySnapshot {
    pub fn counts(&self) -> ReadModelCounts {
        ReadModelCounts {
            accounts: self.accounts.len(),
            messages: self.messages.len(),
            drafts: self.drafts.len(),
            contacts: self.contacts.len(),
            developer_tokens: self.developer_tokens.len(),
            sync_policies: self.sync_policies.len(),
            mailbox_sync_states: self.mailbox_sync_states.len(),
            sync_jobs: self.sync_jobs.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ServiceErrorCode, ServiceErrorView};

    #[test]
    fn serializes_stable_camel_case_error_contract() {
        let value = serde_json::to_value(ServiceErrorView {
            code: ServiceErrorCode::UnsupportedSchema,
            message: "unsupported".into(),
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"code": "unsupportedSchema", "message": "unsupported"})
        );
    }
}
