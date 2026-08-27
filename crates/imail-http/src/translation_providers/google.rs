use std::time::Duration;

use imail_protocol::{TranslatedSegment, TranslationSegment};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use super::{send_json_with_retry, ProviderExecutionError};

const GOOGLE_V2_ENDPOINT: &str = "https://translation.googleapis.com/language/translate/v2";
const GOOGLE_V3_ENDPOINT: &str = "https://translate.googleapis.com";
const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const MAX_BATCH_CHARACTERS: usize = 25_000;
const MAX_BATCH_SEGMENTS: usize = 128;

pub(crate) enum GoogleCredential<'a> {
    ApiKey(&'a str),
    ServiceAccountJson(&'a str),
}

pub(crate) struct GoogleTranslationRequest<'a> {
    pub credential: GoogleCredential<'a>,
    pub project_id: &'a str,
    pub location: Option<&'a str>,
    pub source_language: Option<&'a str>,
    pub target_language: &'a str,
    pub segments: &'a [TranslationSegment],
}

pub(crate) struct GoogleClient {
    agent: ureq::Agent,
    v2_endpoint: String,
    v3_endpoint: String,
    token_endpoint: String,
}

impl Default for GoogleClient {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(45))
                .timeout_write(Duration::from_secs(20))
                .build(),
            v2_endpoint: GOOGLE_V2_ENDPOINT.into(),
            v3_endpoint: GOOGLE_V3_ENDPOINT.into(),
            token_endpoint: GOOGLE_TOKEN_ENDPOINT.into(),
        }
    }
}

impl GoogleClient {
    #[cfg(test)]
    fn api_key_test(endpoint: String) -> Self {
        Self {
            agent: ureq::AgentBuilder::new().build(),
            v2_endpoint: endpoint,
            v3_endpoint: GOOGLE_V3_ENDPOINT.into(),
            token_endpoint: GOOGLE_TOKEN_ENDPOINT.into(),
        }
    }

    pub(crate) fn translate(
        &self,
        request: GoogleTranslationRequest<'_>,
    ) -> Result<Vec<TranslatedSegment>, ProviderExecutionError> {
        match request.credential {
            GoogleCredential::ApiKey(api_key) => self.translate_v2(api_key, request),
            GoogleCredential::ServiceAccountJson(json) => {
                let token = self.service_account_token(json)?;
                self.translate_v3(&token, request)
            }
        }
    }

    fn translate_v2(
        &self,
        api_key: &str,
        request: GoogleTranslationRequest<'_>,
    ) -> Result<Vec<TranslatedSegment>, ProviderExecutionError> {
        let source_language = request.source_language.map(google_language);
        let target_language = google_language(request.target_language);
        let mut translated = Vec::with_capacity(request.segments.len());
        for range in batch_ranges(request.segments) {
            let segments = &request.segments[range];
            let body = GoogleV2Body {
                q: segments
                    .iter()
                    .map(|segment| segment.text.as_str())
                    .collect(),
                source: source_language.as_deref(),
                target: &target_language,
                format: "text",
            };
            let response = send_json_with_retry(
                &self.agent,
                &self.v2_endpoint,
                &[("x-goog-api-key", api_key)],
                &body,
            )
            .map_err(map_http_error)?
            .into_json::<GoogleV2Response>()
            .map_err(|_| ProviderExecutionError::InvalidResponse)?;
            if response.data.translations.len() != segments.len() {
                return Err(ProviderExecutionError::InvalidResponse);
            }
            translated.extend(segments.iter().zip(response.data.translations).map(
                |(segment, value)| TranslatedSegment {
                    id: segment.id.clone(),
                    text: value.translated_text,
                },
            ));
        }
        Ok(translated)
    }

    fn translate_v3(
        &self,
        access_token: &str,
        request: GoogleTranslationRequest<'_>,
    ) -> Result<Vec<TranslatedSegment>, ProviderExecutionError> {
        let location = request.location.unwrap_or("global");
        let source_language = request.source_language.map(google_language);
        let target_language = google_language(request.target_language);
        let endpoint = format!(
            "{}/v3/projects/{}/locations/{}:translateText",
            self.v3_endpoint.trim_end_matches('/'),
            request.project_id,
            location
        );
        let mut translated = Vec::with_capacity(request.segments.len());
        for range in batch_ranges(request.segments) {
            let segments = &request.segments[range];
            let body = GoogleV3Body {
                contents: segments
                    .iter()
                    .map(|segment| segment.text.as_str())
                    .collect(),
                source_language_code: source_language.as_deref(),
                target_language_code: &target_language,
                mime_type: "text/plain",
            };
            let authorization = format!("Bearer {access_token}");
            let response = send_json_with_retry(
                &self.agent,
                &endpoint,
                &[("Authorization", &authorization)],
                &body,
            )
            .map_err(map_http_error)?
            .into_json::<GoogleV3Response>()
            .map_err(|_| ProviderExecutionError::InvalidResponse)?;
            if response.translations.len() != segments.len() {
                return Err(ProviderExecutionError::InvalidResponse);
            }
            translated.extend(segments.iter().zip(response.translations).map(
                |(segment, value)| TranslatedSegment {
                    id: segment.id.clone(),
                    text: value.translated_text,
                },
            ));
        }
        Ok(translated)
    }

    fn service_account_token(&self, raw: &str) -> Result<String, ProviderExecutionError> {
        let account: ServiceAccount =
            serde_json::from_str(raw).map_err(|_| ProviderExecutionError::InvalidConfiguration)?;
        if account.token_uri != GOOGLE_TOKEN_ENDPOINT
            || self.token_endpoint != GOOGLE_TOKEN_ENDPOINT
            || account.client_email.trim().is_empty()
        {
            return Err(ProviderExecutionError::InvalidConfiguration);
        }
        let now = chrono::Utc::now().timestamp();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = account.private_key_id;
        let assertion = jsonwebtoken::encode(
            &header,
            &ServiceAccountClaims {
                iss: &account.client_email,
                scope: "https://www.googleapis.com/auth/cloud-translation",
                aud: GOOGLE_TOKEN_ENDPOINT,
                iat: now,
                exp: now + 3600,
            },
            &EncodingKey::from_rsa_pem(account.private_key.as_bytes())
                .map_err(|_| ProviderExecutionError::InvalidConfiguration)?,
        )
        .map_err(|_| ProviderExecutionError::InvalidConfiguration)?;
        let form = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer")
            .append_pair("assertion", &assertion)
            .finish();
        self.agent
            .post(&self.token_endpoint)
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&form)
            .map_err(map_http_error)?
            .into_json::<TokenResponse>()
            .map(|response| response.access_token)
            .map_err(|_| ProviderExecutionError::InvalidResponse)
    }
}

#[derive(Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    private_key_id: Option<String>,
    token_uri: String,
}

#[derive(Serialize)]
struct ServiceAccountClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Serialize)]
struct GoogleV2Body<'a> {
    q: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a str>,
    target: &'a str,
    format: &'static str,
}

#[derive(Deserialize)]
struct GoogleV2Response {
    data: GoogleV2Data,
}

#[derive(Deserialize)]
struct GoogleV2Data {
    translations: Vec<GoogleV2Translation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleV2Translation {
    translated_text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleV3Body<'a> {
    contents: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_language_code: Option<&'a str>,
    target_language_code: &'a str,
    mime_type: &'static str,
}

#[derive(Deserialize)]
struct GoogleV3Response {
    translations: Vec<GoogleV3Translation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleV3Translation {
    translated_text: String,
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

fn google_language(language: &str) -> String {
    match language.replace('_', "-").to_ascii_lowercase().as_str() {
        "zh-hans" | "zh-cn" => "zh-CN".into(),
        "zh-hant" | "zh-tw" | "zh-hk" => "zh-TW".into(),
        _ => language.to_string(),
    }
}

fn map_http_error(error: ureq::Error) -> ProviderExecutionError {
    match error {
        ureq::Error::Status(400, _) => ProviderExecutionError::UnsupportedLanguage,
        ureq::Error::Status(401 | 403, _) => ProviderExecutionError::Authentication,
        ureq::Error::Status(413, _) => ProviderExecutionError::RequestTooLarge,
        ureq::Error::Status(429, _) => ProviderExecutionError::RateLimited,
        ureq::Error::Status(status, _) if status >= 500 => ProviderExecutionError::Unavailable,
        ureq::Error::Status(_, _) | ureq::Error::Transport(_) => {
            ProviderExecutionError::Unavailable
        }
    }
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
    fn api_key_stays_in_header_and_results_keep_segment_ids() {
        let captured = Arc::new(Mutex::new(String::new()));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let capture = captured.clone();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let size = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..size]);
                let Some(headers_end) = request.windows(4).position(|value| value == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..headers_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= headers_end + 4 + content_length {
                    break;
                }
            }
            *capture.lock().unwrap() = String::from_utf8_lossy(&request).into();
            let body = r#"{"data":{"translations":[{"translatedText":"你好"}]}}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            stream.flush().unwrap();
        });
        let segments = [TranslationSegment {
            id: "s1".into(),
            kind: TranslationSegmentKind::Paragraph,
            text: "Hello".into(),
        }];
        let result = GoogleClient::api_key_test(endpoint)
            .translate(GoogleTranslationRequest {
                credential: GoogleCredential::ApiKey("google-secret"),
                project_id: "project",
                location: None,
                source_language: Some("en"),
                target_language: "zh-CN",
                segments: &segments,
            })
            .unwrap();
        assert_eq!(result[0].id, "s1");
        assert_eq!(result[0].text, "你好");
        let request = captured.lock().unwrap();
        assert!(request
            .to_ascii_lowercase()
            .contains("x-goog-api-key: google-secret"));
        assert!(!request.lines().next().unwrap().contains("google-secret"));
        assert!(request.contains(r#""target":"zh-CN""#));
    }

    #[test]
    fn rejects_service_account_json_with_non_google_token_endpoint() {
        let error = GoogleClient::default()
            .service_account_token(r#"{"client_email":"a@example.test","private_key":"bad","token_uri":"https://attacker.test/token"}"#)
            .unwrap_err();
        assert!(matches!(
            error,
            ProviderExecutionError::InvalidConfiguration
        ));
    }
}
