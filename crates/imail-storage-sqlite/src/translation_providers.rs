use imail_core::{TranslationProviderRecord, TranslationProviderRepository};
use imail_protocol::{
    TranslationCredentialKind, TranslationCredentialReference, TranslationProviderConfiguration,
    TranslationProviderConsent, TranslationProviderProfile,
};
use rusqlite::{params, OptionalExtension};

use crate::{AuthStoreError, SqliteAuthStore};

impl TranslationProviderRepository for SqliteAuthStore {
    fn translation_provider(
        &self,
        user_id: &str,
        profile_id: &str,
    ) -> Result<Option<TranslationProviderRecord>, Self::Error> {
        self.connection
            .query_row(
                "SELECT id, user_id, display_name, execution_target, provider_json,
                        credential_kind, encrypted_credential, enabled, consent_revision,
                        consent_accepted_at, created_at, updated_at
                 FROM translation_provider_profiles WHERE id=?1 AND user_id=?2",
                (profile_id, user_id),
                map_record,
            )
            .optional()
            .map_err(Into::into)
    }

    fn list_translation_providers(
        &self,
        user_id: &str,
    ) -> Result<Vec<TranslationProviderRecord>, Self::Error> {
        self.connection
            .prepare(
                "SELECT id, user_id, display_name, execution_target, provider_json,
                        credential_kind, encrypted_credential, enabled, consent_revision,
                        consent_accepted_at, created_at, updated_at
                 FROM translation_provider_profiles WHERE user_id=?1 ORDER BY created_at, id",
            )?
            .query_map([user_id], map_record)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn upsert_translation_provider(
        &mut self,
        record: &TranslationProviderRecord,
    ) -> Result<(), Self::Error> {
        let existing_owner: Option<String> = self
            .connection
            .query_row(
                "SELECT user_id FROM translation_provider_profiles WHERE id=?1",
                [&record.profile.id],
                |row| row.get(0),
            )
            .optional()?;
        if existing_owner
            .as_deref()
            .is_some_and(|owner| owner != record.owner_id)
        {
            return Err(AuthStoreError::TranslationProfileOwnershipViolation);
        }
        let user_exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM app_users WHERE id=?1)",
            [&record.owner_id],
            |row| row.get(0),
        )?;
        if !user_exists {
            return Err(AuthStoreError::UserNotFound);
        }
        let execution_target = enum_text(record.profile.execution_target)?;
        let provider_json = serde_json::to_string(&record.profile.provider)?;
        let credential_kind = record
            .profile
            .credential
            .as_ref()
            .map(|credential| enum_text(credential.kind))
            .transpose()?;
        let (consent_revision, consent_accepted_at) = record
            .consent
            .as_ref()
            .map(|consent| {
                (
                    Some(consent.disclosure_revision.as_str()),
                    Some(consent.accepted_at.as_str()),
                )
            })
            .unwrap_or((None, None));
        self.connection.execute(
            "INSERT INTO translation_provider_profiles
             (id,user_id,display_name,execution_target,provider_json,credential_kind,
              encrypted_credential,enabled,consent_revision,consent_accepted_at,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(id) DO UPDATE SET
               display_name=excluded.display_name,
               execution_target=excluded.execution_target,
               provider_json=excluded.provider_json,
               credential_kind=excluded.credential_kind,
               encrypted_credential=excluded.encrypted_credential,
               enabled=excluded.enabled,
               consent_revision=excluded.consent_revision,
               consent_accepted_at=excluded.consent_accepted_at,
               updated_at=excluded.updated_at
             WHERE translation_provider_profiles.user_id=excluded.user_id",
            params![
                record.profile.id,
                record.owner_id,
                record.profile.display_name,
                execution_target,
                provider_json,
                credential_kind,
                record.encrypted_credential,
                record.profile.enabled,
                consent_revision,
                consent_accepted_at,
                record.created_at,
                record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn delete_translation_provider(
        &mut self,
        user_id: &str,
        profile_id: &str,
    ) -> Result<bool, Self::Error> {
        Ok(self.connection.execute(
            "DELETE FROM translation_provider_profiles WHERE id=?1 AND user_id=?2",
            (profile_id, user_id),
        )? == 1)
    }
}

fn map_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranslationProviderRecord> {
    let id: String = row.get(0)?;
    let owner_id: String = row.get(1)?;
    let execution_target = parse_enum(row, 3)?;
    let provider: TranslationProviderConfiguration = parse_json(row, 4)?;
    let credential_kind: Option<String> = row.get(5)?;
    let encrypted_credential: Option<String> = row.get(6)?;
    let credential = credential_kind
        .map(|kind| {
            parse_enum_text::<TranslationCredentialKind>(&kind).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()?
        .map(|kind| TranslationCredentialReference {
            id: id.clone(),
            kind,
        });
    let consent_revision: Option<String> = row.get(8)?;
    let consent_accepted_at: Option<String> = row.get(9)?;
    let consent = match (consent_revision, consent_accepted_at) {
        (Some(disclosure_revision), Some(accepted_at)) => Some(TranslationProviderConsent {
            profile_id: id.clone(),
            disclosure_revision,
            accepted_at,
        }),
        (None, None) => None,
        _ => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                8,
                rusqlite::types::Type::Text,
                "翻译服务隐私授权记录不完整".into(),
            ));
        }
    };
    Ok(TranslationProviderRecord {
        owner_id,
        profile: TranslationProviderProfile {
            id,
            display_name: row.get(2)?,
            execution_target,
            provider,
            credential,
            enabled: row.get(7)?,
        },
        consent,
        encrypted_credential,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn enum_text(value: impl serde::Serialize) -> Result<String, AuthStoreError> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            AuthStoreError::Json(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "枚举未序列化为字符串",
            )))
        })
}

fn parse_enum<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let value: String = row.get(index)?;
    parse_enum_text(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn parse_enum_text<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, serde_json::Error> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
}

fn parse_json<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let value: String = row.get(index)?;
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}
