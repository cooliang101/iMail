use std::time::Duration;

use imail_protocol::{TranslatedSegment, TranslationSegment};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{send_json_with_retry, ProviderExecutionError};

const MAX_BATCH_CHARACTERS: usize = 45_000;
const MAX_BATCH_SEGMENTS: usize = 1_000;

pub(crate) struct AzureTranslationRequest<'a> {
    pub api_key: &'a str,
    pub endpoint: &'a str,
    pub region: Option<&'a str>,
    pub source_language: Option<&'a str>,
    pub target_language: &'a str,
    pub segments: &'a [TranslationSegment],
}

pub(crate) struct AzureClient {
    agent: ureq::Agent,
}

impl Default for AzureClient {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(30))
                .timeout_write(Duration::from_secs(20))
                .build(),
        }
    }
}

impl AzureClient {
    pub(crate) fn translate(
        &self,
        request: AzureTranslationRequest<'_>,
    ) -> Result<Vec<TranslatedSegment>, ProviderExecutionError> {
        let endpoint = translate_url(
            request.endpoint,
            request.source_language,
            request.target_language,
        )?;
        let mut translated = Vec::with_capacity(request.segments.len());
        for range in batch_ranges(request.segments) {
            let segments = &request.segments[range];
            let body = segments
                .iter()
                .map(|segment| AzureInput {
                    text: segment.text.as_str(),
                })
                .collect::<Vec<_>>();
            let mut headers = vec![("Ocp-Apim-Subscription-Key", request.api_key)];
            if let Some(region) = request.region {
                headers.push(("Ocp-Apim-Subscription-Region", region));
            }
            let response = send_json_with_retry(&self.agent, endpoint.as_str(), &headers, &body)
                .map_err(|error| map_http_error(*error))?
                .into_json::<Vec<AzureResult>>()
                .map_err(|_| ProviderExecutionError::InvalidResponse)?;
            if response.len() != segments.len() {
                return Err(ProviderExecutionError::InvalidResponse);
            }
            for (segment, mut value) in segments.iter().zip(response) {
                if value.translations.len() != 1 {
                    return Err(ProviderExecutionError::InvalidResponse);
                }
                translated.push(TranslatedSegment {
                    id: segment.id.clone(),
                    text: value.translations.remove(0).text,
                });
            }
        }
        Ok(translated)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct AzureInput<'a> {
    text: &'a str,
}

#[derive(Deserialize)]
struct AzureResult {
    translations: Vec<AzureTranslation>,
}

#[derive(Deserialize)]
struct AzureTranslation {
    text: String,
}

fn translate_url(
    endpoint: &str,
    source_language: Option<&str>,
    target_language: &str,
) -> Result<Url, ProviderExecutionError> {
    let mut url = Url::parse(endpoint).map_err(|_| ProviderExecutionError::InvalidConfiguration)?;
    if url.scheme() != "https" && !cfg!(test) {
        return Err(ProviderExecutionError::InvalidConfiguration);
    }
    if url.username() != "" || url.password().is_some() || url.host_str().is_none() {
        return Err(ProviderExecutionError::InvalidConfiguration);
    }
    let global = url.host_str() == Some("api.cognitive.microsofttranslator.com");
    let path = url.path().trim_end_matches('/');
    let translated_path = if path.is_empty() || path == "/" {
        if global {
            "/translate".to_string()
        } else {
            "/translator/text/v3.0/translate".to_string()
        }
    } else if path.ends_with("/translate") {
        path.to_string()
    } else {
        format!("{path}/translate")
    };
    url.set_path(&translated_path);
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("api-version", "3.0")
        .append_pair("to", target_language);
    if let Some(source) = source_language {
        url.query_pairs_mut().append_pair("from", source);
    }
    Ok(url)
}

fn batch_ranges(segments: &[TranslationSegment]) -> Vec<std::ops::Range<usize>> {
    let mut output = Vec::new();
    let mut start = 0;
    while start < segments.len() {
        let mut end = start;
        let mut characters = 0;
        while end < segments.len() && end - start < MAX_BATCH_SEGMENTS {
            let next = segments[end].text.chars().count();
            if end > start && characters + next > MAX_BATCH_CHARACTERS {
                break;
            }
            characters += next;
            end += 1;
        }
        output.push(start..end);
        start = end;
    }
    output
}

fn map_http_error(error: ureq::Error) -> ProviderExecutionError {
    match error {
        ureq::Error::Status(400, _) => ProviderExecutionError::UnsupportedLanguage,
        ureq::Error::Status(401, _) => ProviderExecutionError::Authentication,
        ureq::Error::Status(403, _) => ProviderExecutionError::QuotaExceeded,
        ureq::Error::Status(408 | 429, _) => ProviderExecutionError::RateLimited,
        ureq::Error::Status(413, _) => ProviderExecutionError::RequestTooLarge,
        ureq::Error::Status(status, _) if status >= 500 => ProviderExecutionError::Unavailable,
        ureq::Error::Status(_, _) | ureq::Error::Transport(_) => {
            ProviderExecutionError::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_global_and_custom_domain_paths_without_credentials() {
        let global = translate_url(
            "https://api.cognitive.microsofttranslator.com",
            Some("en"),
            "zh-Hans",
        )
        .unwrap();
        assert_eq!(global.path(), "/translate");
        assert!(global.as_str().contains("api-version=3.0"));
        assert!(global.as_str().contains("from=en"));

        let custom =
            translate_url("https://example.cognitiveservices.azure.com", None, "de").unwrap();
        assert_eq!(custom.path(), "/translator/text/v3.0/translate");
    }
}
