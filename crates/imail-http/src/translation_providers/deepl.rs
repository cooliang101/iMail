use std::{thread, time::Duration};

use imail_protocol::{DeepLApiPlan, TranslatedSegment, TranslationSegment};
use serde::{Deserialize, Serialize};

const REQUEST_BODY_LIMIT: usize = 128 * 1024;
const SAFE_REQUEST_BODY_LIMIT: usize = 120 * 1024;
const MAX_TEXTS_PER_BATCH: usize = 50;

#[derive(Debug, Clone)]
pub(crate) struct DeepLTranslationRequest<'a> {
    pub api_key: &'a str,
    pub plan: DeepLApiPlan,
    pub source_language: Option<&'a str>,
    pub target_language: &'a str,
    pub segments: &'a [TranslationSegment],
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProviderExecutionError {
    #[error("翻译服务凭据无效或无权访问")]
    Authentication,
    #[error("翻译服务额度已用尽")]
    QuotaExceeded,
    #[error("翻译服务请求过于频繁，请稍后重试")]
    RateLimited,
    #[error("翻译服务暂时不可用，请稍后重试")]
    Unavailable,
    #[error("翻译服务不支持所选语言")]
    UnsupportedLanguage,
    #[error("翻译内容过大，无法发送")]
    RequestTooLarge,
    #[error("翻译服务返回了无效结果")]
    InvalidResponse,
}

impl ProviderExecutionError {
    pub(crate) const fn status(&self) -> u16 {
        match self {
            Self::Authentication => 403,
            Self::QuotaExceeded | Self::RateLimited => 429,
            Self::UnsupportedLanguage => 400,
            Self::RequestTooLarge => 413,
            Self::Unavailable | Self::InvalidResponse => 502,
        }
    }

    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Authentication => "TRANSLATION_PROVIDER_AUTH_FAILED",
            Self::QuotaExceeded => "TRANSLATION_PROVIDER_QUOTA_EXCEEDED",
            Self::RateLimited => "TRANSLATION_PROVIDER_RATE_LIMITED",
            Self::Unavailable => "TRANSLATION_PROVIDER_UNAVAILABLE",
            Self::UnsupportedLanguage => "TRANSLATION_LANGUAGE_UNSUPPORTED",
            Self::RequestTooLarge => "TRANSLATION_REQUEST_TOO_LARGE",
            Self::InvalidResponse => "TRANSLATION_PROVIDER_RESPONSE_INVALID",
        }
    }

    pub(crate) const fn message(&self) -> &'static str {
        match self {
            Self::Authentication => "翻译服务凭据无效或无权访问",
            Self::QuotaExceeded => "翻译服务额度已用尽",
            Self::RateLimited => "翻译服务请求过于频繁，请稍后重试",
            Self::Unavailable => "翻译服务暂时不可用，请稍后重试",
            Self::UnsupportedLanguage => "翻译服务不支持所选语言",
            Self::RequestTooLarge => "翻译内容过大，无法发送",
            Self::InvalidResponse => "翻译服务返回了无效结果",
        }
    }
}

pub(crate) struct DeepLClient {
    agent: ureq::Agent,
    free_base_url: String,
    pro_base_url: String,
    retry_delays: Vec<Duration>,
}

impl Default for DeepLClient {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(45))
                .timeout_write(Duration::from_secs(20))
                .build(),
            free_base_url: "https://api-free.deepl.com".into(),
            pro_base_url: "https://api.deepl.com".into(),
            retry_delays: vec![Duration::from_millis(250), Duration::from_millis(750)],
        }
    }
}

impl DeepLClient {
    #[cfg(test)]
    fn for_test(base_url: String, retry_delays: Vec<Duration>) -> Self {
        Self {
            agent: ureq::AgentBuilder::new().build(),
            free_base_url: base_url.clone(),
            pro_base_url: base_url,
            retry_delays,
        }
    }

    pub(crate) fn translate(
        &self,
        request: DeepLTranslationRequest<'_>,
    ) -> Result<Vec<TranslatedSegment>, ProviderExecutionError> {
        let source_language = request
            .source_language
            .map(map_source_language)
            .transpose()?;
        let target_language = map_target_language(request.target_language)?;
        let base_url = match request.plan {
            DeepLApiPlan::Free => &self.free_base_url,
            DeepLApiPlan::Pro => &self.pro_base_url,
        };
        let mut translated = Vec::with_capacity(request.segments.len());
        for batch in batches(
            request.segments,
            source_language.as_deref(),
            &target_language,
        )? {
            let response = self.send_batch(
                &format!("{}/v2/translate", base_url.trim_end_matches('/')),
                request.api_key,
                &batch.body,
            )?;
            if response.translations.len() != batch.segment_ids.len() {
                return Err(ProviderExecutionError::InvalidResponse);
            }
            translated.extend(
                batch
                    .segment_ids
                    .into_iter()
                    .zip(response.translations)
                    .map(|(id, value)| TranslatedSegment {
                        id,
                        text: value.text,
                    }),
            );
        }
        Ok(translated)
    }

    fn send_batch(
        &self,
        url: &str,
        api_key: &str,
        body: &str,
    ) -> Result<DeepLResponse, ProviderExecutionError> {
        for attempt in 0..=self.retry_delays.len() {
            let response = self
                .agent
                .post(url)
                .set("Authorization", &format!("DeepL-Auth-Key {api_key}"))
                .set("Content-Type", "application/json")
                .send_string(body);
            match response {
                Ok(response) => {
                    return response
                        .into_json::<DeepLResponse>()
                        .map_err(|_| ProviderExecutionError::InvalidResponse)
                }
                Err(ureq::Error::Status(status, _)) if status == 429 || status >= 500 => {
                    if let Some(delay) = self.retry_delays.get(attempt) {
                        thread::sleep(*delay);
                        continue;
                    }
                    return Err(if status == 429 {
                        ProviderExecutionError::RateLimited
                    } else {
                        ProviderExecutionError::Unavailable
                    });
                }
                Err(ureq::Error::Status(401 | 403, _)) => {
                    return Err(ProviderExecutionError::Authentication)
                }
                Err(ureq::Error::Status(456, _)) => {
                    return Err(ProviderExecutionError::QuotaExceeded)
                }
                Err(ureq::Error::Status(413, _)) => {
                    return Err(ProviderExecutionError::RequestTooLarge)
                }
                Err(ureq::Error::Status(400, _)) => {
                    return Err(ProviderExecutionError::UnsupportedLanguage)
                }
                Err(ureq::Error::Status(_, _)) => return Err(ProviderExecutionError::Unavailable),
                Err(ureq::Error::Transport(error)) => {
                    let _ = &error;
                    #[cfg(test)]
                    eprintln!("DeepL test transport error: {error}");
                    return Err(ProviderExecutionError::Unavailable);
                }
            }
        }
        Err(ProviderExecutionError::Unavailable)
    }
}

struct DeepLBatch {
    segment_ids: Vec<String>,
    body: String,
}

#[derive(Serialize)]
struct DeepLBody<'a> {
    text: Vec<&'a str>,
    target_lang: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_lang: Option<&'a str>,
    preserve_formatting: bool,
}

#[derive(Deserialize)]
struct DeepLResponse {
    translations: Vec<DeepLTranslation>,
}

#[derive(Deserialize)]
struct DeepLTranslation {
    text: String,
}

fn batches(
    segments: &[TranslationSegment],
    source_language: Option<&str>,
    target_language: &str,
) -> Result<Vec<DeepLBatch>, ProviderExecutionError> {
    let mut output = Vec::new();
    let mut start = 0;
    while start < segments.len() {
        let mut end = start;
        let mut accepted = None;
        while end < segments.len() && end - start < MAX_TEXTS_PER_BATCH {
            end += 1;
            let body = serde_json::to_string(&DeepLBody {
                text: segments[start..end]
                    .iter()
                    .map(|segment| segment.text.as_str())
                    .collect(),
                target_lang: target_language,
                source_lang: source_language,
                preserve_formatting: true,
            })
            .map_err(|_| ProviderExecutionError::InvalidResponse)?;
            if body.len() > SAFE_REQUEST_BODY_LIMIT {
                break;
            }
            accepted = Some((end, body));
        }
        let (next, body) = accepted.ok_or(ProviderExecutionError::RequestTooLarge)?;
        debug_assert!(body.len() < REQUEST_BODY_LIMIT);
        output.push(DeepLBatch {
            segment_ids: segments[start..next]
                .iter()
                .map(|segment| segment.id.clone())
                .collect(),
            body,
        });
        start = next;
    }
    Ok(output)
}

fn map_source_language(language: &str) -> Result<String, ProviderExecutionError> {
    let primary = language
        .split('-')
        .next()
        .unwrap_or(language)
        .to_ascii_uppercase();
    match primary.as_str() {
        "AR" | "BG" | "CS" | "DA" | "DE" | "EL" | "EN" | "ES" | "ET" | "FI" | "FR" | "HU"
        | "ID" | "IT" | "JA" | "KO" | "LT" | "LV" | "NB" | "NL" | "PL" | "PT" | "RO" | "RU"
        | "SK" | "SL" | "SV" | "TR" | "UK" | "ZH" => Ok(primary),
        _ => Err(ProviderExecutionError::UnsupportedLanguage),
    }
}

fn map_target_language(language: &str) -> Result<String, ProviderExecutionError> {
    let normalized = language.replace('_', "-").to_ascii_lowercase();
    let mapped = match normalized.as_str() {
        "zh-hans" | "zh-cn" => "ZH-HANS",
        "zh-hant" | "zh-tw" | "zh-hk" => "ZH-HANT",
        "en" => "EN-US",
        "pt" => "PT-PT",
        _ => return map_source_language(language),
    };
    Ok(mapped.into())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    use imail_protocol::TranslationSegmentKind;

    use super::*;

    #[test]
    fn preserves_segment_order_and_keeps_api_key_in_authorization_header() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server = mock_server(
            vec![(200, r#"{"translations":[{"text":"你好"},{"text":"世界"}]}"#)],
            requests.clone(),
        );
        let segments = vec![segment("a", "Hello"), segment("b", "World")];
        let result = DeepLClient::for_test(server, vec![])
            .translate(DeepLTranslationRequest {
                api_key: "private-key:fx",
                plan: DeepLApiPlan::Free,
                source_language: Some("en-US"),
                target_language: "zh-Hans",
                segments: &segments,
            })
            .unwrap();
        assert_eq!(result[0].id, "a");
        assert_eq!(result[1].text, "世界");
        let request = &requests.lock().unwrap()[0];
        assert!(request.contains("Authorization: DeepL-Auth-Key private-key:fx"));
        assert!(request.contains(r#""target_lang":"ZH-HANS""#));
        assert!(!request
            .split("\r\n\r\n")
            .last()
            .unwrap()
            .contains("private-key"));
    }

    #[test]
    fn retries_transient_failure_and_maps_quota_without_exposing_body() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server = mock_server(
            vec![
                (500, r#"{"message":"secret upstream detail"}"#),
                (456, "quota"),
            ],
            requests.clone(),
        );
        let error = DeepLClient::for_test(server, vec![Duration::ZERO])
            .translate(DeepLTranslationRequest {
                api_key: "key",
                plan: DeepLApiPlan::Pro,
                source_language: None,
                target_language: "de",
                segments: &[segment("a", "Hello")],
            })
            .unwrap_err();
        assert!(matches!(error, ProviderExecutionError::QuotaExceeded));
        assert_eq!(requests.lock().unwrap().len(), 2);
        assert!(!error.to_string().contains("secret"));
    }

    fn segment(id: &str, text: &str) -> TranslationSegment {
        TranslationSegment {
            id: id.into(),
            kind: TranslationSegmentKind::Paragraph,
            text: text.into(),
        }
    }

    fn mock_server(
        responses: Vec<(u16, &'static str)>,
        requests: Arc<Mutex<Vec<String>>>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = Vec::new();
                let mut chunk = [0; 4096];
                loop {
                    let size = stream.read(&mut chunk).unwrap();
                    buffer.extend_from_slice(&chunk[..size]);
                    let Some(headers_end) =
                        buffer.windows(4).position(|value| value == b"\r\n\r\n")
                    else {
                        continue;
                    };
                    let headers = String::from_utf8_lossy(&buffer[..headers_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if buffer.len() >= headers_end + 4 + content_length {
                        break;
                    }
                }
                requests
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer).into_owned());
                let reason = if status == 200 { "OK" } else { "Error" };
                write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                stream.flush().unwrap();
            }
        });
        format!("http://{address}")
    }
}
