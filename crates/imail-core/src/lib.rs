use std::{collections::BTreeMap, error::Error};

use imail_protocol::{
    AppUserView, ContactReadModel, DatabaseInventory, DeveloperTokenReadModel, DraftReadModel,
    MessageReadModel, RateLimitDecision, ReadOnlySnapshot,
};
use serde_json::Value;

pub mod accounts;
pub mod authentication;
pub mod authorization_export;
pub mod contacts;
pub mod developer_tokens;
pub mod drafts;
pub mod external_access;
pub mod mail_operations;
pub mod maintenance;
pub mod messages;
pub mod notifications;
pub mod oauth_accounts;
pub mod oauth_refresh;
pub mod preferences;
pub mod privacy;
pub mod sync_execution;
pub mod sync_runtime;
pub mod theme;
pub mod translation_settings;
pub mod translations;

#[derive(Debug, thiserror::Error)]
pub enum ApplicationError<E: Error + Send + Sync + 'static> {
    #[error("{message}")]
    Domain {
        code: &'static str,
        status: u16,
        message: &'static str,
    },
    #[error(transparent)]
    Repository(E),
}

impl<E: Error + Send + Sync + 'static> ApplicationError<E> {
    pub fn code(&self) -> &str {
        match self {
            Self::Domain { code, .. } => code,
            Self::Repository(_) => "STORAGE_ERROR",
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::Domain { status, .. } => *status,
            Self::Repository(_) => 500,
        }
    }
}

pub trait LocalRepository:
    AccountRepository + ContentRepository<Error = <Self as AccountRepository>::Error>
{
}

pub trait PrivacyRepository {
    type Error: Error + Send + Sync + 'static;

    fn clear_user_mail_data(
        &mut self,
        user_id: &str,
    ) -> Result<imail_protocol::ClearMailDataResult, Self::Error>;
}

impl<T> LocalRepository for T where
    T: AccountRepository + ContentRepository<Error = <T as AccountRepository>::Error>
{
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedDeveloperToken {
    pub token: DeveloperTokenReadModel,
    pub raw: String,
}

#[derive(Clone, PartialEq)]
pub struct AccountRecord {
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
    pub encrypted_secret: String,
    pub auth_method: Option<String>,
    pub created_at: String,
    pub last_sync_at: Option<String>,
    pub status: String,
    pub last_error: Option<String>,
    pub mailboxes: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoFetchAttemptRecord {
    pub owner_id: String,
    pub target: String,
    pub domain_key: String,
    pub status: String,
    pub detail: String,
    pub attempted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationProviderRecord {
    pub owner_id: String,
    pub profile: imail_protocol::TranslationProviderProfile,
    pub consent: Option<imail_protocol::TranslationProviderConsent>,
    pub encrypted_credential: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub trait TranslationProviderRepository: AccountRepository {
    fn translation_provider(
        &self,
        user_id: &str,
        profile_id: &str,
    ) -> Result<Option<TranslationProviderRecord>, Self::Error>;
    fn list_translation_providers(
        &self,
        user_id: &str,
    ) -> Result<Vec<TranslationProviderRecord>, Self::Error>;
    fn upsert_translation_provider(
        &mut self,
        record: &TranslationProviderRecord,
    ) -> Result<(), Self::Error>;
    fn delete_translation_provider(
        &mut self,
        user_id: &str,
        profile_id: &str,
    ) -> Result<bool, Self::Error>;
}

pub trait TranslationCacheRepository {
    type Error: Error + Send + Sync + 'static;

    fn translation_artifact(
        &self,
        key: &imail_protocol::TranslationCacheKey,
    ) -> Result<Option<imail_protocol::TranslationArtifact>, Self::Error>;
    fn upsert_translation_artifact(
        &mut self,
        artifact: &imail_protocol::TranslationArtifact,
    ) -> Result<(), Self::Error>;
    fn clear_translation_artifacts(&mut self, user_id: &str) -> Result<u64, Self::Error>;
}

pub trait ReadOnlyRepository {
    type Error: Error + Send + Sync + 'static;

    fn inventory(&self) -> Result<DatabaseInventory, Self::Error>;
    fn read_snapshot(&self) -> Result<ReadOnlySnapshot, Self::Error>;
}

pub trait AuthRepository {
    type Error: Error + Send + Sync + 'static;

    fn create_user(
        &mut self,
        login: &str,
        display_name: &str,
        password: &str,
    ) -> Result<AppUserView, Self::Error>;
    fn setup_required(&self) -> Result<bool, Self::Error>;
    fn create_user_if_allowed(
        &mut self,
        login: &str,
        display_name: &str,
        password: &str,
        registration_open: bool,
    ) -> Result<Option<AppUserView>, Self::Error>;
    fn authenticate(&self, login: &str, password: &str)
        -> Result<Option<AppUserView>, Self::Error>;
    fn verify_user_password(&self, user_id: &str, password: &str) -> Result<bool, Self::Error>;
    fn create_session(&mut self, user_id: &str) -> Result<String, Self::Error>;
    fn user_for_session(&mut self, raw_session: &str) -> Result<Option<AppUserView>, Self::Error>;
    fn delete_session(&mut self, raw_session: &str) -> Result<(), Self::Error>;
    fn consume_attempt(
        &mut self,
        key: &str,
        maximum: u32,
        window_ms: u64,
    ) -> Result<RateLimitDecision, Self::Error>;
    fn clear_attempt(&mut self, key: &str) -> Result<(), Self::Error>;
    fn record_security_event(
        &mut self,
        event_type: &str,
        actor: &str,
        user_id: Option<&str>,
        detail: &BTreeMap<String, String>,
    ) -> Result<(), Self::Error>;
}

pub trait DeveloperTokenRepository {
    type Error: Error + Send + Sync + 'static;

    fn issue_developer_token(
        &mut self,
        user_id: &str,
        name: &str,
        scopes: &[String],
        requested_account_ids: &[String],
        ttl_seconds: u64,
    ) -> Result<IssuedDeveloperToken, Self::Error>;
    fn authenticate_developer_token(
        &mut self,
        raw: &str,
        required_scope: &str,
    ) -> Result<Option<DeveloperTokenReadModel>, Self::Error>;
    fn list_developer_tokens(
        &self,
        user_id: &str,
    ) -> Result<Vec<DeveloperTokenReadModel>, Self::Error>;
    fn revoke_developer_token(
        &mut self,
        user_id: &str,
        token_id: &str,
    ) -> Result<bool, Self::Error>;
}

pub trait AccountRepository {
    type Error: Error + Send + Sync + 'static;

    fn account(
        &self,
        user_id: &str,
        account_id: &str,
    ) -> Result<Option<AccountRecord>, Self::Error>;
    fn list_accounts(&self, user_id: &str) -> Result<Vec<AccountRecord>, Self::Error>;
    fn insert_account_if_email_available(
        &mut self,
        account: &AccountRecord,
    ) -> Result<bool, Self::Error>;
    fn upsert_account(&mut self, account: &AccountRecord) -> Result<(), Self::Error>;
    fn delete_account(&mut self, user_id: &str, account_id: &str) -> Result<bool, Self::Error>;
    fn user_metadata(&self, user_id: &str, key: &str) -> Result<Option<String>, Self::Error>;
    fn set_user_metadata(
        &mut self,
        user_id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), Self::Error>;
}

pub trait ContentRepository {
    type Error: Error + Send + Sync + 'static;

    fn list_messages(&self, user_id: &str) -> Result<Vec<MessageReadModel>, Self::Error>;
    fn upsert_message(
        &mut self,
        user_id: &str,
        message: &MessageReadModel,
    ) -> Result<(), Self::Error>;
    fn delete_message(&mut self, user_id: &str, message_id: &str) -> Result<bool, Self::Error>;
    fn list_drafts(&self, user_id: &str) -> Result<Vec<DraftReadModel>, Self::Error>;
    fn upsert_draft(&mut self, user_id: &str, draft: &DraftReadModel) -> Result<(), Self::Error>;
    fn delete_draft(&mut self, user_id: &str, draft_id: &str) -> Result<bool, Self::Error>;
    fn list_contacts(&self, user_id: &str) -> Result<Vec<ContactReadModel>, Self::Error>;
    fn upsert_contact(&mut self, contact: &ContactReadModel) -> Result<(), Self::Error>;
    fn replace_contacts(
        &mut self,
        user_id: &str,
        contacts: &[ContactReadModel],
    ) -> Result<(), Self::Error>;
    fn delete_contact(&mut self, user_id: &str, address: &str) -> Result<bool, Self::Error>;
    fn list_logo_fetch_attempts(
        &self,
        user_id: &str,
    ) -> Result<Vec<LogoFetchAttemptRecord>, Self::Error>;
    fn upsert_logo_fetch_attempt(
        &mut self,
        attempt: &LogoFetchAttemptRecord,
    ) -> Result<(), Self::Error>;
    fn delete_logo_fetch_attempt(
        &mut self,
        user_id: &str,
        target: &str,
    ) -> Result<bool, Self::Error>;
}

pub trait MessageRepository {
    type Error: Error + Send + Sync + 'static;

    fn query_messages(
        &self,
        user_id: &str,
        query: &messages::MessageQuery,
        now: &str,
    ) -> Result<messages::MessagePage, Self::Error>;
    fn message(
        &self,
        user_id: &str,
        message_id: &str,
    ) -> Result<Option<MessageReadModel>, Self::Error>;
    fn message_source(
        &self,
        user_id: &str,
        message_id: &str,
    ) -> Result<Option<Vec<u8>>, Self::Error>;
    fn message_stats(
        &self,
        user_id: &str,
        now: &str,
    ) -> Result<messages::MessageStats, Self::Error>;
    fn query_gateway_messages(
        &self,
        user_id: &str,
        query: &messages::GatewayMessageQuery,
    ) -> Result<messages::GatewayMessagePage, Self::Error>;
}
