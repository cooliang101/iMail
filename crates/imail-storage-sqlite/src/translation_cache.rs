use imail_core::TranslationCacheRepository;
use imail_protocol::{TranslatedSegment, TranslationArtifact, TranslationCacheKey};
use rusqlite::{params, OptionalExtension};

use crate::{AuthStoreError, SqliteAuthStore};

impl TranslationCacheRepository for SqliteAuthStore {
    type Error = AuthStoreError;

    fn translation_artifact(
        &self,
        key: &TranslationCacheKey,
    ) -> Result<Option<TranslationArtifact>, Self::Error> {
        self.connection
            .query_row(
                "SELECT translated_segments_json,created_at,updated_at
                 FROM message_translation_cache
                 WHERE user_id=?1 AND message_id=?2 AND body_hash=?3 AND source_language=?4
                   AND target_language=?5 AND profile_id=?6 AND provider_revision=?7
                   AND segment_version=?8",
                params![
                    key.user_id,
                    key.message_id,
                    key.body_hash,
                    key.source_language.as_deref().unwrap_or(""),
                    key.target_language,
                    key.profile_id,
                    key.provider_revision,
                    key.segment_version,
                ],
                |row| {
                    let raw: String = row.get(0)?;
                    let segments =
                        serde_json::from_str::<Vec<TranslatedSegment>>(&raw).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })?;
                    Ok(TranslationArtifact {
                        key: key.clone(),
                        segments,
                        created_at: row.get(1)?,
                        updated_at: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    fn upsert_translation_artifact(
        &mut self,
        artifact: &TranslationArtifact,
    ) -> Result<(), Self::Error> {
        let owned: bool = self.connection.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM messages m JOIN accounts a ON a.id=m.account_id
               WHERE m.id=?1 AND a.user_id=?2
             )",
            (&artifact.key.message_id, &artifact.key.user_id),
            |row| row.get(0),
        )?;
        if !owned {
            return Err(AuthStoreError::TranslationCacheOwnershipViolation);
        }
        let segments = serde_json::to_string(&artifact.segments)?;
        self.connection.execute(
            "INSERT INTO message_translation_cache
             (user_id,message_id,body_hash,source_language,target_language,profile_id,
              provider_revision,segment_version,translated_segments_json,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(user_id,message_id,body_hash,source_language,target_language,profile_id,
                         provider_revision,segment_version)
             DO UPDATE SET translated_segments_json=excluded.translated_segments_json,
                           updated_at=excluded.updated_at",
            params![
                artifact.key.user_id,
                artifact.key.message_id,
                artifact.key.body_hash,
                artifact.key.source_language.as_deref().unwrap_or(""),
                artifact.key.target_language,
                artifact.key.profile_id,
                artifact.key.provider_revision,
                artifact.key.segment_version,
                segments,
                artifact.created_at,
                artifact.updated_at,
            ],
        )?;
        Ok(())
    }

    fn clear_translation_artifacts(&mut self, user_id: &str) -> Result<u64, Self::Error> {
        Ok(self.connection.execute(
            "DELETE FROM message_translation_cache WHERE user_id=?1",
            [user_id],
        )? as u64)
    }
}
