use std::collections::HashMap;

use imail_protocol::{
    TranslatedSegment, TranslationArtifact, TranslationCacheKey, TranslationDocument,
    TranslationPreparationRequest, TranslationPreparationView, TranslationProviderStatus,
    TranslationSegment, TranslationSegmentKind, TRANSLATION_SEGMENT_VERSION,
};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::{
    translation_settings::{descriptor, profile_view, translation_preferences},
    ApplicationError, MessageRepository, TranslationCacheRepository, TranslationProviderRepository,
};

const MAX_TRANSLATION_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_SEGMENT_CHARACTERS: usize = 4_000;

pub struct TranslationService<'a, R> {
    repository: &'a mut R,
}

impl<'a, R> TranslationService<'a, R>
where
    R: MessageRepository
        + TranslationProviderRepository<Error = <R as MessageRepository>::Error>
        + TranslationCacheRepository<Error = <R as MessageRepository>::Error>,
{
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn prepare(
        &self,
        user_id: &str,
        message_id: &str,
        request: TranslationPreparationRequest,
    ) -> Result<TranslationPreparationView, ApplicationError<<R as MessageRepository>::Error>> {
        validate_language(request.source_language.as_deref())?;
        validate_language(Some(&request.target_language))?;
        let message = self
            .repository
            .message(user_id, message_id)
            .map_err(ApplicationError::Repository)?
            .ok_or_else(|| domain("MESSAGE_NOT_FOUND", 404, "邮件不存在"))?;
        let record = self
            .repository
            .translation_provider(user_id, &request.profile_id)
            .map_err(ApplicationError::Repository)?
            .ok_or_else(|| domain("TRANSLATION_PROFILE_NOT_FOUND", 404, "翻译服务配置不存在"))?;
        let profile = profile_view(record);
        if matches!(
            profile.status,
            TranslationProviderStatus::Disabled
                | TranslationProviderStatus::NeedsCredential
                | TranslationProviderStatus::NeedsConsent
        ) {
            return Err(domain(
                "TRANSLATION_PROFILE_NOT_READY",
                409,
                "翻译服务尚未就绪",
            ));
        }
        let provider_revision = descriptor(profile.profile.provider.kind()).provider_revision;
        let document = segment_message_body(&message.id, &message.text, message.html.as_deref())?;
        if document.segments.is_empty() {
            return Err(domain(
                "TRANSLATION_BODY_EMPTY",
                409,
                "邮件没有可翻译的正文",
            ));
        }
        let key = TranslationCacheKey {
            user_id: user_id.to_string(),
            message_id: message.id,
            body_hash: document.body_hash.clone(),
            source_language: request.source_language,
            target_language: request.target_language,
            profile_id: profile.profile.id.clone(),
            provider_revision: provider_revision.clone(),
            segment_version: document.segment_version,
        };
        let cached = if translation_preferences(self.repository, user_id)?.cache_translations {
            self.repository
                .translation_artifact(&key)
                .map_err(ApplicationError::Repository)?
        } else {
            None
        };
        Ok(TranslationPreparationView {
            document,
            profile,
            provider_revision,
            cache_key: key,
            cached,
        })
    }

    pub fn store_artifact(
        &mut self,
        preparation: &TranslationPreparationView,
        segments: Vec<TranslatedSegment>,
        created_at: &str,
        updated_at: &str,
    ) -> Result<TranslationArtifact, ApplicationError<<R as MessageRepository>::Error>> {
        validate_translated_segments(&preparation.document, &segments)?;
        let artifact = TranslationArtifact {
            key: preparation.cache_key.clone(),
            segments,
            created_at: created_at.to_string(),
            updated_at: updated_at.to_string(),
        };
        if translation_preferences(self.repository, &artifact.key.user_id)?.cache_translations {
            self.repository
                .upsert_translation_artifact(&artifact)
                .map_err(ApplicationError::Repository)?;
        }
        Ok(artifact)
    }

    pub fn clear_cache(
        &mut self,
        user_id: &str,
    ) -> Result<u64, ApplicationError<<R as MessageRepository>::Error>> {
        self.repository
            .clear_translation_artifacts(user_id)
            .map_err(ApplicationError::Repository)
    }
}

pub fn segment_message_body<E: std::error::Error + Send + Sync + 'static>(
    message_id: &str,
    text: &str,
    html: Option<&str>,
) -> Result<TranslationDocument, ApplicationError<E>> {
    let source = if text.trim().is_empty() {
        html.map(html_to_plain_text).unwrap_or_default()
    } else {
        normalize_text(text)
    };
    if source.len() > MAX_TRANSLATION_BODY_BYTES {
        return Err(domain(
            "TRANSLATION_BODY_TOO_LARGE",
            413,
            "邮件正文过大，无法翻译",
        ));
    }
    let (visible, omitted_quoted_text) = omit_quoted_history(&source);
    let body_hash = format!("{:x}", Sha256::digest(visible.as_bytes()));
    let mut occurrences = HashMap::<String, usize>::new();
    let mut segments = Vec::new();
    for block in visible
        .split("\n\n")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        for chunk in split_long_segment(block, MAX_SEGMENT_CHARACTERS) {
            let kind = if chunk.lines().all(is_list_line) {
                TranslationSegmentKind::ListItem
            } else {
                TranslationSegmentKind::Paragraph
            };
            let digest = format!("{:x}", Sha256::digest(chunk.as_bytes()));
            let occurrence = occurrences.entry(digest.clone()).or_insert(0);
            *occurrence += 1;
            segments.push(TranslationSegment {
                id: format!("s-{}-{}", &digest[..16], occurrence),
                kind,
                text: chunk,
            });
        }
    }
    Ok(TranslationDocument {
        message_id: message_id.to_string(),
        body_hash,
        segment_version: TRANSLATION_SEGMENT_VERSION,
        omitted_quoted_text,
        segments,
    })
}

fn validate_translated_segments<E: std::error::Error + Send + Sync + 'static>(
    document: &TranslationDocument,
    translated: &[TranslatedSegment],
) -> Result<(), ApplicationError<E>> {
    if translated.len() != document.segments.len()
        || translated
            .iter()
            .zip(&document.segments)
            .any(|(translated, source)| {
                translated.id != source.id || translated.text.trim().is_empty()
            })
    {
        return Err(domain(
            "TRANSLATION_RESULT_INVALID",
            502,
            "翻译服务返回了不完整的分段结果",
        ));
    }
    Ok(())
}

fn omit_quoted_history(source: &str) -> (String, bool) {
    let quote_marker = Regex::new(
        r"(?i)^(?:on .+ wrote:|在.+写道[:：]|-{2,}\s*original message\s*-{2,}|发件人[:：]\s*.+)$",
    )
    .expect("quoted history marker is valid");
    let mut visible = Vec::new();
    let mut omitted = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if quote_marker.is_match(trimmed) {
            omitted = true;
            break;
        }
        if trimmed.starts_with('>') {
            omitted = true;
            continue;
        }
        visible.push(line);
    }
    (normalize_text(&visible.join("\n")), omitted)
}

fn split_long_segment(value: &str, maximum: usize) -> Vec<String> {
    if value.chars().count() <= maximum {
        return vec![value.to_string()];
    }
    let mut output = Vec::new();
    let mut current = String::new();
    for sentence in value.split_inclusive(['。', '！', '？', '.', '!', '?', '\n']) {
        if !current.is_empty() && current.chars().count() + sentence.chars().count() > maximum {
            output.push(current.trim().to_string());
            current.clear();
        }
        if sentence.chars().count() > maximum {
            for character in sentence.chars() {
                current.push(character);
                if current.chars().count() == maximum {
                    output.push(std::mem::take(&mut current));
                }
            }
        } else {
            current.push_str(sentence);
        }
    }
    if !current.trim().is_empty() {
        output.push(current.trim().to_string());
    }
    output
}

fn is_list_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed
            .split_once('.')
            .is_some_and(|(prefix, _)| prefix.bytes().all(|byte| byte.is_ascii_digit()))
}

fn html_to_plain_text(html: &str) -> String {
    let without_hidden = Regex::new(
        r"(?is)<(?:head|style|script|template|noscript|svg|canvas|object)\b[^>]*>.*?</(?:head|style|script|template|noscript|svg|canvas|object)\s*>",
    )
    .expect("hidden html regex is valid")
    .replace_all(html, "");
    let breaks = Regex::new(
        r"(?i)<br\s*/?>|</(?:p|div|section|article|header|footer|li|tr|blockquote|h[1-6])\s*>",
    )
    .expect("html break regex is valid")
    .replace_all(&without_hidden, "\n");
    let stripped = Regex::new(r"(?s)<[^>]+>")
        .expect("html tag regex is valid")
        .replace_all(&breaks, "");
    normalize_text(&decode_entities(&stripped))
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn normalize_text(value: &str) -> String {
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>();
    Regex::new(r"\n[ \t]+|[ \t]+\n")
        .expect("whitespace regex is valid")
        .replace_all(&lines.join("\n"), "\n")
        .trim()
        .to_string()
}

fn validate_language<E: std::error::Error + Send + Sync + 'static>(
    language: Option<&str>,
) -> Result<(), ApplicationError<E>> {
    if language.is_some_and(|language| {
        language.len() < 2
            || language.len() > 35
            || !language
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }) {
        return Err(domain("TRANSLATION_LANGUAGE_INVALID", 400, "翻译语言无效"));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_visible_text_with_stable_ids_and_omits_reply_history() {
        let source = "Hello team.\n\n- first\n- second\n\nOn Tue, Alex wrote:\n> old message";
        let first = segment_message_body::<std::io::Error>("message-1", source, None).unwrap();
        let second = segment_message_body::<std::io::Error>("message-1", source, None).unwrap();
        assert_eq!(first, second);
        assert!(first.omitted_quoted_text);
        assert_eq!(first.segments.len(), 2);
        assert_eq!(first.segments[1].kind, TranslationSegmentKind::ListItem);
        assert!(!serde_json::to_string(&first)
            .unwrap()
            .contains("old message"));
    }

    #[test]
    fn extracts_html_only_messages_without_active_content() {
        let document = segment_message_body::<std::io::Error>(
            "message-2",
            "",
            Some("<style>secret</style><p>Hello &amp; welcome</p><script>bad()</script>"),
        )
        .unwrap();
        assert_eq!(document.segments[0].text, "Hello & welcome");
    }
}
