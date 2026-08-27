use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranslationProviderKind {
    EdgeLocal,
    #[serde(rename = "deepl")]
    DeepL,
    GoogleCloud,
    AzureTranslator,
    BingWeb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranslationExecutionTarget {
    WebView,
    LocalService,
    RemoteService,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeepLApiPlan {
    Free,
    Pro,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum TranslationProviderConfiguration {
    EdgeLocal,
    #[serde(rename = "deepl")]
    DeepL {
        plan: DeepLApiPlan,
    },
    GoogleCloud {
        project_id: String,
        location: Option<String>,
    },
    AzureTranslator {
        endpoint: String,
        region: Option<String>,
    },
    BingWeb {
        market: Option<String>,
    },
}

impl TranslationProviderConfiguration {
    pub const fn kind(&self) -> TranslationProviderKind {
        match self {
            Self::EdgeLocal => TranslationProviderKind::EdgeLocal,
            Self::DeepL { .. } => TranslationProviderKind::DeepL,
            Self::GoogleCloud { .. } => TranslationProviderKind::GoogleCloud,
            Self::AzureTranslator { .. } => TranslationProviderKind::AzureTranslator,
            Self::BingWeb { .. } => TranslationProviderKind::BingWeb,
        }
    }

    pub const fn requires_credentials(&self) -> bool {
        matches!(
            self,
            Self::DeepL { .. } | Self::GoogleCloud { .. } | Self::AzureTranslator { .. }
        )
    }

    pub const fn sends_content_off_device(&self) -> bool {
        !matches!(self, Self::EdgeLocal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranslationCredentialKind {
    #[serde(rename = "deepl-api-key")]
    DeepLApiKey,
    GoogleApiKey,
    GoogleServiceAccount,
    AzureApiKey,
}

/// A safe reference to encrypted credential material. The secret itself is never part of the
/// shared protocol model and must not be returned to WebView, HTTP, or MCP callers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationCredentialReference {
    pub id: String,
    pub kind: TranslationCredentialKind,
}

/// One configured translation provider in one concrete execution environment.
///
/// Provider profiles are environment-scoped. For example, a desktop-local DeepL profile and a
/// remote-service DeepL profile are distinct even when they belong to the same user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationProviderProfile {
    pub id: String,
    pub display_name: String,
    pub execution_target: TranslationExecutionTarget,
    pub provider: TranslationProviderConfiguration,
    pub credential: Option<TranslationCredentialReference>,
    pub enabled: bool,
}

/// Preferences that are safe to synchronize between iMail environments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationPreferences {
    /// `None` follows the iMail interface language. A value uses a canonical BCP-47 tag.
    pub default_target_language: Option<String>,
    pub auto_translate: bool,
    pub cache_translations: bool,
}

impl Default for TranslationPreferences {
    fn default() -> Self {
        Self {
            default_target_language: None,
            auto_translate: false,
            cache_translations: true,
        }
    }
}

/// Selection that is local to the active WebView/local-service/remote-service environment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationEnvironmentPreferences {
    pub default_profile_id: Option<String>,
}

/// Records explicit consent for a profile that sends message content off the device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationProviderConsent {
    pub profile_id: String,
    pub disclosure_revision: String,
    pub accepted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationProviderDescriptor {
    pub kind: TranslationProviderKind,
    pub display_name: String,
    pub execution_targets: Vec<TranslationExecutionTarget>,
    pub credential_kinds: Vec<TranslationCredentialKind>,
    pub sends_content_off_device: bool,
    pub experimental: bool,
    pub provider_revision: String,
    pub disclosure_revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TranslationProviderStatus {
    Configured,
    Disabled,
    NeedsCredential,
    NeedsConsent,
    RuntimeCheckRequired,
    Experimental,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationProviderProfileView {
    pub profile: TranslationProviderProfile,
    pub status: TranslationProviderStatus,
    pub consent: Option<TranslationProviderConsent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSettingsView {
    pub preferences: TranslationPreferences,
    pub environment: TranslationEnvironmentPreferences,
    pub registry: Vec<TranslationProviderDescriptor>,
    pub profiles: Vec<TranslationProviderProfileView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationProviderProfileInput {
    pub display_name: String,
    pub execution_target: TranslationExecutionTarget,
    pub provider: TranslationProviderConfiguration,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSettingsUpdate {
    pub preferences: TranslationPreferences,
    pub environment: TranslationEnvironmentPreferences,
}

pub const TRANSLATION_SEGMENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranslationSegmentKind {
    Paragraph,
    ListItem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSegment {
    pub id: String,
    pub kind: TranslationSegmentKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationDocument {
    pub message_id: String,
    pub body_hash: String,
    pub segment_version: u32,
    pub omitted_quoted_text: bool,
    pub segments: Vec<TranslationSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatedSegment {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationCacheKey {
    pub user_id: String,
    pub message_id: String,
    pub body_hash: String,
    pub source_language: Option<String>,
    pub target_language: String,
    pub profile_id: String,
    pub provider_revision: String,
    pub segment_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationArtifact {
    pub key: TranslationCacheKey,
    pub segments: Vec<TranslatedSegment>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationPreparationRequest {
    pub profile_id: String,
    pub source_language: Option<String>,
    pub target_language: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationPreparationView {
    pub document: TranslationDocument,
    pub profile: TranslationProviderProfileView,
    pub provider_revision: String,
    pub cache_key: TranslationCacheKey,
    pub cached: Option<TranslationArtifact>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_stable_provider_profile_contract_without_secret_material() {
        let profile = TranslationProviderProfile {
            id: "deepl-local-1".into(),
            display_name: "DeepL Free".into(),
            execution_target: TranslationExecutionTarget::LocalService,
            provider: TranslationProviderConfiguration::DeepL {
                plan: DeepLApiPlan::Free,
            },
            credential: Some(TranslationCredentialReference {
                id: "credential-1".into(),
                kind: TranslationCredentialKind::DeepLApiKey,
            }),
            enabled: true,
        };

        let value = serde_json::to_value(profile).unwrap();
        assert_eq!(value["executionTarget"], "local-service");
        assert_eq!(
            value["provider"],
            serde_json::json!({"type": "deepl", "plan": "free"})
        );
        assert_eq!(value["credential"]["kind"], "deepl-api-key");
        assert!(value.get("apiKey").is_none());
        assert!(value.get("secret").is_none());
    }

    #[test]
    fn classifies_local_and_network_providers() {
        let local = TranslationProviderConfiguration::EdgeLocal;
        let remote = TranslationProviderConfiguration::AzureTranslator {
            endpoint: "https://example.cognitiveservices.azure.com".into(),
            region: Some("westus".into()),
        };

        assert_eq!(local.kind(), TranslationProviderKind::EdgeLocal);
        assert!(!local.requires_credentials());
        assert!(!local.sends_content_off_device());
        assert_eq!(remote.kind(), TranslationProviderKind::AzureTranslator);
        assert!(remote.requires_credentials());
        assert!(remote.sends_content_off_device());
    }

    #[test]
    fn defaults_to_manual_translation_and_local_cache() {
        let preferences = TranslationPreferences::default();
        assert_eq!(preferences.default_target_language, None);
        assert!(!preferences.auto_translate);
        assert!(preferences.cache_translations);
    }

    #[test]
    fn provider_configuration_uses_camel_case_fields() {
        let value = serde_json::to_value(TranslationProviderConfiguration::GoogleCloud {
            project_id: "project-1".into(),
            location: Some("global".into()),
        })
        .unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "type": "google-cloud",
                "projectId": "project-1",
                "location": "global"
            })
        );
    }

    #[test]
    fn settings_view_does_not_define_credential_secret_fields() {
        let field_names = serde_json::to_value(TranslationSettingsView {
            preferences: TranslationPreferences::default(),
            environment: TranslationEnvironmentPreferences::default(),
            registry: Vec::new(),
            profiles: Vec::new(),
        })
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
        assert_eq!(
            field_names,
            ["environment", "preferences", "profiles", "registry"]
        );
    }
}
