use imail_protocol::{
    TranslationCredentialKind, TranslationCredentialReference, TranslationEnvironmentPreferences,
    TranslationExecutionTarget, TranslationPreferences, TranslationProviderConfiguration,
    TranslationProviderConsent, TranslationProviderDescriptor, TranslationProviderKind,
    TranslationProviderProfile, TranslationProviderProfileInput, TranslationProviderProfileView,
    TranslationProviderStatus, TranslationSettingsUpdate, TranslationSettingsView,
};
use serde_json::json;

use crate::{
    AccountRepository, ApplicationError, TranslationProviderRecord, TranslationProviderRepository,
};

const PREFERENCES_KEY: &str = "translation_preferences_v1";
const ENVIRONMENT_KEY: &str = "translation_environment_v1";
const MAX_SECRET_BYTES: usize = 128 * 1024;

pub(crate) fn translation_preferences<R: AccountRepository>(
    repository: &R,
    user_id: &str,
) -> Result<TranslationPreferences, ApplicationError<R::Error>> {
    Ok(read_metadata(repository, user_id, PREFERENCES_KEY)?
        .unwrap_or_else(TranslationPreferences::default))
}

#[derive(Debug, thiserror::Error)]
#[error("翻译凭据加密或解密失败")]
pub struct TranslationCredentialCodecError;

pub trait TranslationCredentialCodec {
    fn encrypt(&self, value: &serde_json::Value)
        -> Result<String, TranslationCredentialCodecError>;
    fn decrypt(&self, payload: &str) -> Result<serde_json::Value, TranslationCredentialCodecError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationCredentialSecret {
    pub kind: TranslationCredentialKind,
    pub secret: String,
}

pub struct TranslationSettingsService<'a, R: TranslationProviderRepository> {
    repository: &'a mut R,
}

impl<'a, R: TranslationProviderRepository> TranslationSettingsService<'a, R> {
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn read(
        &self,
        user_id: &str,
    ) -> Result<TranslationSettingsView, ApplicationError<R::Error>> {
        let preferences = translation_preferences(self.repository, user_id)?;
        let mut environment = read_metadata(self.repository, user_id, ENVIRONMENT_KEY)?
            .unwrap_or_else(TranslationEnvironmentPreferences::default);
        let records = self
            .repository
            .list_translation_providers(user_id)
            .map_err(ApplicationError::Repository)?;
        if environment.default_profile_id.as_ref().is_some_and(|id| {
            !records
                .iter()
                .any(|record| record.profile.id == *id && default_profile_ready(record))
        }) {
            environment.default_profile_id = None;
        }
        Ok(TranslationSettingsView {
            preferences,
            environment,
            registry: provider_registry(),
            profiles: records.into_iter().map(profile_view).collect(),
        })
    }

    pub fn update(
        &mut self,
        user_id: &str,
        update: TranslationSettingsUpdate,
    ) -> Result<TranslationSettingsView, ApplicationError<R::Error>> {
        validate_preferences(&update.preferences)?;
        if let Some(profile_id) = update.environment.default_profile_id.as_deref() {
            let profile = self
                .repository
                .translation_provider(user_id, profile_id)
                .map_err(ApplicationError::Repository)?
                .ok_or_else(|| {
                    domain("TRANSLATION_PROFILE_NOT_FOUND", 404, "翻译服务配置不存在")
                })?;
            let status = profile_view(profile).status;
            if matches!(
                status,
                TranslationProviderStatus::Disabled
                    | TranslationProviderStatus::NeedsCredential
                    | TranslationProviderStatus::NeedsConsent
            ) {
                return Err(domain(
                    "TRANSLATION_PROFILE_NOT_READY",
                    409,
                    "不能选择尚未就绪的翻译服务",
                ));
            }
        }
        store_metadata(
            self.repository,
            user_id,
            PREFERENCES_KEY,
            &update.preferences,
        )?;
        store_metadata(
            self.repository,
            user_id,
            ENVIRONMENT_KEY,
            &update.environment,
        )?;
        self.read(user_id)
    }

    pub fn upsert_profile(
        &mut self,
        user_id: &str,
        profile_id: &str,
        input: TranslationProviderProfileInput,
        now: &str,
    ) -> Result<TranslationSettingsView, ApplicationError<R::Error>> {
        validate_profile(profile_id, &input)?;
        let existing = self
            .repository
            .translation_provider(user_id, profile_id)
            .map_err(ApplicationError::Repository)?;
        if existing.is_none()
            && input.provider.kind() == TranslationProviderKind::EdgeLocal
            && self
                .repository
                .list_translation_providers(user_id)
                .map_err(ApplicationError::Repository)?
                .iter()
                .any(|record| record.profile.provider.kind() == TranslationProviderKind::EdgeLocal)
        {
            return Err(domain(
                "TRANSLATION_EDGE_PROFILE_ALREADY_EXISTS",
                409,
                "Edge 本地翻译只支持一个配置",
            ));
        }
        let provider_changed = existing
            .as_ref()
            .is_some_and(|record| record.profile.provider != input.provider);
        let allowed_credentials = descriptor(input.provider.kind()).credential_kinds;
        let credential = existing
            .as_ref()
            .and_then(|record| record.profile.credential.clone())
            .filter(|credential| allowed_credentials.contains(&credential.kind));
        let encrypted_credential = existing
            .as_ref()
            .and_then(|record| record.encrypted_credential.clone())
            .filter(|_| credential.is_some());
        let record = TranslationProviderRecord {
            owner_id: user_id.to_string(),
            profile: TranslationProviderProfile {
                id: profile_id.to_string(),
                display_name: input.display_name.trim().to_string(),
                execution_target: input.execution_target,
                provider: input.provider,
                credential,
                enabled: input.enabled,
            },
            consent: existing.as_ref().and_then(|record| {
                (!provider_changed)
                    .then(|| record.consent.clone())
                    .flatten()
            }),
            encrypted_credential,
            created_at: existing
                .as_ref()
                .map(|record| record.created_at.clone())
                .unwrap_or_else(|| now.to_string()),
            updated_at: now.to_string(),
        };
        self.repository
            .upsert_translation_provider(&record)
            .map_err(ApplicationError::Repository)?;
        self.read(user_id)
    }

    pub fn set_credential(
        &mut self,
        user_id: &str,
        profile_id: &str,
        credential: TranslationCredentialSecret,
        codec: &dyn TranslationCredentialCodec,
        now: &str,
    ) -> Result<TranslationSettingsView, ApplicationError<R::Error>> {
        let mut record = self
            .repository
            .translation_provider(user_id, profile_id)
            .map_err(ApplicationError::Repository)?
            .ok_or_else(|| domain("TRANSLATION_PROFILE_NOT_FOUND", 404, "翻译服务配置不存在"))?;
        if !descriptor(record.profile.provider.kind())
            .credential_kinds
            .contains(&credential.kind)
        {
            return Err(domain(
                "TRANSLATION_CREDENTIAL_KIND_INVALID",
                400,
                "凭据类型与翻译服务不匹配",
            ));
        }
        let secret = credential.secret.trim();
        if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
            return Err(domain(
                "TRANSLATION_CREDENTIAL_INVALID",
                400,
                "翻译服务凭据无效",
            ));
        }
        let encrypted = codec
            .encrypt(&json!({"kind": credential.kind, "secret": secret}))
            .map_err(|_| {
                domain(
                    "TRANSLATION_CREDENTIAL_ENCRYPT_FAILED",
                    500,
                    "无法安全保存翻译服务凭据",
                )
            })?;
        record.profile.credential = Some(TranslationCredentialReference {
            id: profile_id.to_string(),
            kind: credential.kind,
        });
        record.encrypted_credential = Some(encrypted);
        record.updated_at = now.to_string();
        self.repository
            .upsert_translation_provider(&record)
            .map_err(ApplicationError::Repository)?;
        self.read(user_id)
    }

    pub fn clear_credential(
        &mut self,
        user_id: &str,
        profile_id: &str,
        now: &str,
    ) -> Result<TranslationSettingsView, ApplicationError<R::Error>> {
        let mut record = self
            .repository
            .translation_provider(user_id, profile_id)
            .map_err(ApplicationError::Repository)?
            .ok_or_else(|| domain("TRANSLATION_PROFILE_NOT_FOUND", 404, "翻译服务配置不存在"))?;
        record.profile.credential = None;
        record.encrypted_credential = None;
        record.updated_at = now.to_string();
        self.repository
            .upsert_translation_provider(&record)
            .map_err(ApplicationError::Repository)?;
        self.read(user_id)
    }

    pub fn credential_secret(
        &self,
        user_id: &str,
        profile_id: &str,
        expected_kind: TranslationCredentialKind,
        codec: &dyn TranslationCredentialCodec,
    ) -> Result<String, ApplicationError<R::Error>> {
        let record = self
            .repository
            .translation_provider(user_id, profile_id)
            .map_err(ApplicationError::Repository)?
            .ok_or_else(|| domain("TRANSLATION_PROFILE_NOT_FOUND", 404, "翻译服务配置不存在"))?;
        let reference = record.profile.credential.as_ref().ok_or_else(|| {
            domain(
                "TRANSLATION_CREDENTIAL_REQUIRED",
                409,
                "翻译服务缺少访问凭据",
            )
        })?;
        if reference.kind != expected_kind {
            return Err(domain(
                "TRANSLATION_CREDENTIAL_KIND_INVALID",
                409,
                "翻译服务凭据类型不匹配",
            ));
        }
        let encrypted = record.encrypted_credential.as_deref().ok_or_else(|| {
            domain(
                "TRANSLATION_CREDENTIAL_REQUIRED",
                409,
                "翻译服务缺少访问凭据",
            )
        })?;
        let value = codec.decrypt(encrypted).map_err(|_| {
            domain(
                "TRANSLATION_CREDENTIAL_DECRYPT_FAILED",
                500,
                "无法读取翻译服务凭据",
            )
        })?;
        let kind = serde_json::from_value::<TranslationCredentialKind>(
            value
                .get("kind")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )
        .map_err(|_| domain("TRANSLATION_CREDENTIAL_INVALID", 500, "翻译服务凭据无效"))?;
        let secret = value
            .get("secret")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|secret| !secret.is_empty())
            .ok_or_else(|| domain("TRANSLATION_CREDENTIAL_INVALID", 500, "翻译服务凭据无效"))?;
        if kind != expected_kind {
            return Err(domain(
                "TRANSLATION_CREDENTIAL_KIND_INVALID",
                500,
                "翻译服务凭据类型不匹配",
            ));
        }
        Ok(secret.to_string())
    }

    pub fn accept_consent(
        &mut self,
        user_id: &str,
        profile_id: &str,
        now: &str,
    ) -> Result<TranslationSettingsView, ApplicationError<R::Error>> {
        let mut record = self
            .repository
            .translation_provider(user_id, profile_id)
            .map_err(ApplicationError::Repository)?
            .ok_or_else(|| domain("TRANSLATION_PROFILE_NOT_FOUND", 404, "翻译服务配置不存在"))?;
        let provider = descriptor(record.profile.provider.kind());
        if !provider.sends_content_off_device {
            return Err(domain(
                "TRANSLATION_CONSENT_NOT_REQUIRED",
                409,
                "本地翻译不需要云端数据授权",
            ));
        }
        record.consent = Some(TranslationProviderConsent {
            profile_id: profile_id.to_string(),
            disclosure_revision: provider.disclosure_revision,
            accepted_at: now.to_string(),
        });
        record.updated_at = now.to_string();
        self.repository
            .upsert_translation_provider(&record)
            .map_err(ApplicationError::Repository)?;
        self.read(user_id)
    }

    pub fn revoke_consent(
        &mut self,
        user_id: &str,
        profile_id: &str,
        now: &str,
    ) -> Result<TranslationSettingsView, ApplicationError<R::Error>> {
        let mut record = self
            .repository
            .translation_provider(user_id, profile_id)
            .map_err(ApplicationError::Repository)?
            .ok_or_else(|| domain("TRANSLATION_PROFILE_NOT_FOUND", 404, "翻译服务配置不存在"))?;
        record.consent = None;
        record.updated_at = now.to_string();
        self.repository
            .upsert_translation_provider(&record)
            .map_err(ApplicationError::Repository)?;
        self.read(user_id)
    }

    pub fn delete_profile(
        &mut self,
        user_id: &str,
        profile_id: &str,
    ) -> Result<TranslationSettingsView, ApplicationError<R::Error>> {
        if !self
            .repository
            .delete_translation_provider(user_id, profile_id)
            .map_err(ApplicationError::Repository)?
        {
            return Err(domain(
                "TRANSLATION_PROFILE_NOT_FOUND",
                404,
                "翻译服务配置不存在",
            ));
        }
        let mut environment = read_metadata(self.repository, user_id, ENVIRONMENT_KEY)?
            .unwrap_or_else(TranslationEnvironmentPreferences::default);
        if environment.default_profile_id.as_deref() == Some(profile_id) {
            environment.default_profile_id = None;
            store_metadata(self.repository, user_id, ENVIRONMENT_KEY, &environment)?;
        }
        self.read(user_id)
    }
}

pub fn provider_registry() -> Vec<TranslationProviderDescriptor> {
    vec![
        TranslationProviderDescriptor {
            kind: TranslationProviderKind::EdgeLocal,
            display_name: "Edge 本地翻译".into(),
            execution_targets: vec![TranslationExecutionTarget::WebView],
            credential_kinds: Vec::new(),
            sends_content_off_device: false,
            experimental: false,
            provider_revision: "edge-local-v1".into(),
            disclosure_revision: "edge-local-v1".into(),
        },
        TranslationProviderDescriptor {
            kind: TranslationProviderKind::DeepL,
            display_name: "DeepL".into(),
            execution_targets: vec![
                TranslationExecutionTarget::LocalService,
                TranslationExecutionTarget::RemoteService,
            ],
            credential_kinds: vec![TranslationCredentialKind::DeepLApiKey],
            sends_content_off_device: true,
            experimental: false,
            provider_revision: "deepl-v1".into(),
            disclosure_revision: "deepl-cloud-v1".into(),
        },
        TranslationProviderDescriptor {
            kind: TranslationProviderKind::GoogleCloud,
            display_name: "Google Cloud Translation".into(),
            execution_targets: vec![
                TranslationExecutionTarget::LocalService,
                TranslationExecutionTarget::RemoteService,
            ],
            credential_kinds: vec![
                TranslationCredentialKind::GoogleApiKey,
                TranslationCredentialKind::GoogleServiceAccount,
            ],
            sends_content_off_device: true,
            experimental: false,
            provider_revision: "google-cloud-v1".into(),
            disclosure_revision: "google-cloud-v1".into(),
        },
        TranslationProviderDescriptor {
            kind: TranslationProviderKind::AzureTranslator,
            display_name: "Azure Translator".into(),
            execution_targets: vec![
                TranslationExecutionTarget::LocalService,
                TranslationExecutionTarget::RemoteService,
            ],
            credential_kinds: vec![TranslationCredentialKind::AzureApiKey],
            sends_content_off_device: true,
            experimental: false,
            provider_revision: "azure-translator-v1".into(),
            disclosure_revision: "azure-translator-v1".into(),
        },
        TranslationProviderDescriptor {
            kind: TranslationProviderKind::BingWeb,
            display_name: "Bing 网页翻译".into(),
            execution_targets: vec![
                TranslationExecutionTarget::LocalService,
                TranslationExecutionTarget::RemoteService,
            ],
            credential_kinds: Vec::new(),
            sends_content_off_device: true,
            experimental: true,
            provider_revision: "bing-web-experimental-v1".into(),
            disclosure_revision: "bing-web-experimental-v1".into(),
        },
    ]
}

pub(crate) fn descriptor(kind: TranslationProviderKind) -> TranslationProviderDescriptor {
    provider_registry()
        .into_iter()
        .find(|provider| provider.kind == kind)
        .expect("every provider kind has a descriptor")
}

pub(crate) fn profile_view(record: TranslationProviderRecord) -> TranslationProviderProfileView {
    let status = profile_status(&record);
    TranslationProviderProfileView {
        profile: record.profile,
        status,
        consent: record.consent,
    }
}

fn profile_status(record: &TranslationProviderRecord) -> TranslationProviderStatus {
    let provider = descriptor(record.profile.provider.kind());
    let consent_current = record
        .consent
        .as_ref()
        .is_some_and(|consent| consent.disclosure_revision == provider.disclosure_revision);
    if !record.profile.enabled {
        TranslationProviderStatus::Disabled
    } else if record.profile.provider.requires_credentials() && record.profile.credential.is_none()
    {
        TranslationProviderStatus::NeedsCredential
    } else if provider.sends_content_off_device && !consent_current {
        TranslationProviderStatus::NeedsConsent
    } else if record.profile.provider.kind() == TranslationProviderKind::EdgeLocal {
        TranslationProviderStatus::RuntimeCheckRequired
    } else if provider.experimental {
        TranslationProviderStatus::Experimental
    } else {
        TranslationProviderStatus::Configured
    }
}

fn default_profile_ready(record: &TranslationProviderRecord) -> bool {
    !matches!(
        profile_status(record),
        TranslationProviderStatus::Disabled
            | TranslationProviderStatus::NeedsCredential
            | TranslationProviderStatus::NeedsConsent
    )
}

fn validate_profile<E: std::error::Error + Send + Sync + 'static>(
    profile_id: &str,
    input: &TranslationProviderProfileInput,
) -> Result<(), ApplicationError<E>> {
    if profile_id.is_empty()
        || profile_id.len() > 80
        || !profile_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(domain(
            "TRANSLATION_PROFILE_ID_INVALID",
            400,
            "翻译服务配置 ID 无效",
        ));
    }
    let display_name = input.display_name.trim();
    if display_name.is_empty() || display_name.chars().count() > 80 {
        return Err(domain(
            "TRANSLATION_PROFILE_NAME_INVALID",
            400,
            "翻译服务名称无效",
        ));
    }
    let provider = descriptor(input.provider.kind());
    if !provider.execution_targets.contains(&input.execution_target) {
        return Err(domain(
            "TRANSLATION_EXECUTION_TARGET_INVALID",
            400,
            "翻译服务不能在所选位置运行",
        ));
    }
    match &input.provider {
        TranslationProviderConfiguration::GoogleCloud {
            project_id,
            location,
        } => {
            validate_optional_field(project_id, 160)?;
            validate_optional(location.as_deref(), 80)?;
            if !project_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b':'))
                || location.as_deref().is_some_and(|location| {
                    !location
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                })
            {
                return Err(domain(
                    "TRANSLATION_PROVIDER_CONFIG_INVALID",
                    400,
                    "Google Cloud 项目或区域无效",
                ));
            }
        }
        TranslationProviderConfiguration::AzureTranslator { endpoint, region } => {
            if !endpoint.starts_with("https://") || endpoint.len() > 500 {
                return Err(domain(
                    "TRANSLATION_ENDPOINT_INVALID",
                    400,
                    "Azure Translator 地址必须使用 HTTPS",
                ));
            }
            validate_optional(region.as_deref(), 80)?;
        }
        TranslationProviderConfiguration::BingWeb { market } => {
            validate_optional(market.as_deref(), 40)?;
        }
        TranslationProviderConfiguration::EdgeLocal
        | TranslationProviderConfiguration::DeepL { .. } => {}
    }
    Ok(())
}

fn validate_preferences<E: std::error::Error + Send + Sync + 'static>(
    preferences: &TranslationPreferences,
) -> Result<(), ApplicationError<E>> {
    if let Some(language) = preferences.default_target_language.as_deref() {
        if language.len() < 2
            || language.len() > 35
            || !language
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(domain(
                "TRANSLATION_LANGUAGE_INVALID",
                400,
                "默认翻译语言无效",
            ));
        }
    }
    Ok(())
}

fn validate_optional_field<E: std::error::Error + Send + Sync + 'static>(
    value: &str,
    maximum: usize,
) -> Result<(), ApplicationError<E>> {
    if value.trim().is_empty() || value.len() > maximum {
        return Err(domain(
            "TRANSLATION_PROVIDER_CONFIG_INVALID",
            400,
            "翻译服务配置无效",
        ));
    }
    Ok(())
}

fn validate_optional<E: std::error::Error + Send + Sync + 'static>(
    value: Option<&str>,
    maximum: usize,
) -> Result<(), ApplicationError<E>> {
    if value.is_some_and(|value| value.trim().is_empty() || value.len() > maximum) {
        return Err(domain(
            "TRANSLATION_PROVIDER_CONFIG_INVALID",
            400,
            "翻译服务配置无效",
        ));
    }
    Ok(())
}

fn read_metadata<T: serde::de::DeserializeOwned, R: AccountRepository>(
    repository: &R,
    user_id: &str,
    key: &str,
) -> Result<Option<T>, ApplicationError<R::Error>> {
    repository
        .user_metadata(user_id, key)
        .map_err(ApplicationError::Repository)
        .map(|raw| raw.and_then(|raw| serde_json::from_str(&raw).ok()))
}

fn store_metadata<T: serde::Serialize, R: AccountRepository>(
    repository: &mut R,
    user_id: &str,
    key: &str,
    value: &T,
) -> Result<(), ApplicationError<R::Error>> {
    let encoded = serde_json::to_string(value).expect("translation settings are serializable");
    repository
        .set_user_metadata(user_id, key, &encoded)
        .map_err(ApplicationError::Repository)
}

fn domain<E: std::error::Error + Send + Sync + 'static>(
    code: &'static str,
    status: u16,
    message: &'static str,
) -> ApplicationError<E> {
    ApplicationError::Domain {
        code,
        status,
        message,
    }
}
