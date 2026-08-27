use std::{
    io::Read,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use imail_protocol::{TranslatedSegment, TranslationSegment};
use regex::Regex;
use serde_json::Value;
use url::Url;

use super::ProviderExecutionError;

const HOME_URL: &str = "https://www.bing.com/translator";
const MAX_CHUNK_CHARACTERS: usize = 900;
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
const COOLDOWN: Duration = Duration::from_secs(5 * 60);
static COOLDOWN_UNTIL: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

pub(crate) struct BingWebTranslationRequest<'a> {
    pub market: Option<&'a str>,
    pub source_language: Option<&'a str>,
    pub target_language: &'a str,
    pub segments: &'a [TranslationSegment],
}

pub(crate) struct BingWebClient {
    agent: ureq::Agent,
    home_url: String,
}

impl Default for BingWebClient {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(30))
                .timeout_write(Duration::from_secs(20))
                .redirects(5)
                .build(),
            home_url: HOME_URL.into(),
        }
    }
}

impl BingWebClient {
    pub(crate) fn translate(
        &self,
        request: BingWebTranslationRequest<'_>,
    ) -> Result<Vec<TranslatedSegment>, ProviderExecutionError> {
        if cooldown_active() {
            return Err(ProviderExecutionError::RateLimited);
        }
        let mut session = self.fetch_session(request.market)?;
        let mut output = Vec::with_capacity(request.segments.len());
        for segment in request.segments {
            let mut text = String::new();
            for chunk in split_chunks(&segment.text) {
                match self.translate_chunk(
                    &mut session,
                    chunk,
                    request.source_language.unwrap_or("auto-detect"),
                    request.target_language,
                    request.market,
                ) {
                    Err(BingAttemptError::RefreshSession) => {
                        session = self.fetch_session(request.market)?;
                        text.push_str(
                            &self
                                .translate_chunk(
                                    &mut session,
                                    chunk,
                                    request.source_language.unwrap_or("auto-detect"),
                                    request.target_language,
                                    request.market,
                                )
                                .map_err(map_attempt_error)?,
                        );
                    }
                    Err(error) => return Err(map_attempt_error(error)),
                    Ok(value) => text.push_str(&value),
                }
            }
            output.push(TranslatedSegment {
                id: segment.id.clone(),
                text,
            });
        }
        Ok(output)
    }

    fn fetch_session(&self, market: Option<&str>) -> Result<BingSession, ProviderExecutionError> {
        let accept_language = safe_market(market);
        let response = self
            .agent
            .get(&self.home_url)
            .set("Accept", "text/html,application/xhtml+xml")
            .set("Accept-Language", &accept_language)
            .call()
            .map_err(map_http_error)?;
        let final_url =
            Url::parse(response.get_url()).map_err(|_| ProviderExecutionError::InvalidResponse)?;
        if !cfg!(test)
            && !final_url
                .host_str()
                .is_some_and(|host| host == "bing.com" || host.ends_with(".bing.com"))
        {
            return Err(ProviderExecutionError::InvalidResponse);
        }
        if response
            .header("Content-Length")
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|length| length > MAX_RESPONSE_BYTES)
        {
            return Err(ProviderExecutionError::InvalidResponse);
        }
        let mut body = String::new();
        response
            .into_reader()
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_string(&mut body)
            .map_err(|_| ProviderExecutionError::InvalidResponse)?;
        if body.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(ProviderExecutionError::InvalidResponse);
        }
        parse_session(final_url, &body)
    }

    fn translate_chunk(
        &self,
        session: &mut BingSession,
        text: &str,
        source_language: &str,
        target_language: &str,
        market: Option<&str>,
    ) -> Result<String, BingAttemptError> {
        session.count += 1;
        let accept_language = safe_market(market);
        let mut endpoint = session.base_url.clone();
        endpoint.set_path("/ttranslatev3");
        endpoint
            .query_pairs_mut()
            .append_pair("isVertical", "1")
            .append_pair("IG", &session.ig)
            .append_pair("IID", &format!("{}.{}", session.iid, session.count));
        let form = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("fromLang", source_language)
            .append_pair("to", target_language)
            .append_pair("text", text)
            .append_pair("token", &session.token)
            .append_pair("key", &session.key)
            .finish();
        let response = self
            .agent
            .post(endpoint.as_str())
            .set("Accept", "*/*")
            .set("Accept-Language", &accept_language)
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&form)
            .map_err(|error| match error {
                ureq::Error::Status(401 | 429, _) => {
                    begin_cooldown();
                    BingAttemptError::Provider(ProviderExecutionError::RateLimited)
                }
                other => BingAttemptError::Provider(map_http_error(other)),
            })?;
        let value = response
            .into_json::<Value>()
            .map_err(|_| BingAttemptError::Provider(ProviderExecutionError::InvalidResponse))?;
        let status = value
            .get("StatusCode")
            .or_else(|| value.get("statusCode"))
            .and_then(Value::as_u64)
            .unwrap_or(200);
        if status == 205 {
            return Err(BingAttemptError::RefreshSession);
        }
        if status != 200 {
            return Err(BingAttemptError::Provider(
                ProviderExecutionError::Unavailable,
            ));
        }
        value
            .get(0)
            .and_then(|item| item.get("translations"))
            .and_then(|translations| translations.get(0))
            .and_then(|translation| translation.get("text"))
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_string)
            .ok_or(BingAttemptError::Provider(
                ProviderExecutionError::InvalidResponse,
            ))
    }
}

struct BingSession {
    base_url: Url,
    ig: String,
    iid: String,
    key: String,
    token: String,
    count: u32,
}

enum BingAttemptError {
    RefreshSession,
    Provider(ProviderExecutionError),
}

fn parse_session(base_url: Url, body: &str) -> Result<BingSession, ProviderExecutionError> {
    let ig = Regex::new(r#"IG:\s*"([A-Za-z0-9]+)""#)
        .expect("Bing IG pattern is valid")
        .captures(body)
        .and_then(|capture| capture.get(1))
        .map(|value| value.as_str().to_string());
    let abuse = Regex::new(r#"params_AbusePreventionHelper\s*=\s*\[\s*([0-9]+)\s*,\s*"([^"]+)""#)
        .expect("Bing abuse prevention pattern is valid")
        .captures(body);
    let iid = Regex::new(r#"(?s)id=["']rich_tta["'][^>]*data-iid=["']([^"']+)["']"#)
        .expect("Bing IID pattern is valid")
        .captures(body)
        .and_then(|capture| capture.get(1))
        .map(|value| value.as_str().to_string());
    let (Some(ig), Some(abuse), Some(iid)) = (ig, abuse, iid) else {
        return Err(ProviderExecutionError::InvalidResponse);
    };
    Ok(BingSession {
        base_url,
        ig,
        iid,
        key: abuse[1].to_string(),
        token: abuse[2].to_string(),
        count: 0,
    })
}

fn split_chunks(text: &str) -> Vec<&str> {
    if text.chars().count() <= MAX_CHUNK_CHARACTERS {
        return vec![text];
    }
    let mut output = Vec::new();
    let mut start = 0;
    let mut count = 0;
    for (index, _) in text.char_indices() {
        if count == MAX_CHUNK_CHARACTERS {
            output.push(&text[start..index]);
            start = index;
            count = 0;
        }
        count += 1;
    }
    output.push(&text[start..]);
    output
}

fn safe_market(market: Option<&str>) -> String {
    market
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 35
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        .unwrap_or("zh-CN")
        .to_string()
}

fn cooldown_active() -> bool {
    COOLDOWN_UNTIL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|until| *until)
        .is_some_and(|until| until > Instant::now())
}

fn begin_cooldown() {
    if let Ok(mut until) = COOLDOWN_UNTIL.get_or_init(|| Mutex::new(None)).lock() {
        *until = Some(Instant::now() + COOLDOWN);
    }
}

fn map_attempt_error(error: BingAttemptError) -> ProviderExecutionError {
    match error {
        BingAttemptError::RefreshSession => ProviderExecutionError::Unavailable,
        BingAttemptError::Provider(error) => error,
    }
}

fn map_http_error(error: ureq::Error) -> ProviderExecutionError {
    match error {
        ureq::Error::Status(401 | 429, _) => {
            begin_cooldown();
            ProviderExecutionError::RateLimited
        }
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
    fn parses_current_edge_translate_session_markers() {
        let body = r#"<script>IG:"ABC123"; var params_AbusePreventionHelper = [12345, "token-value", 0];</script><textarea id="rich_tta" data-iid="translator.5024"></textarea>"#;
        let session =
            parse_session(Url::parse("https://cn.bing.com/translator").unwrap(), body).unwrap();
        assert_eq!(session.ig, "ABC123");
        assert_eq!(session.iid, "translator.5024");
        assert_eq!(session.key, "12345");
        assert_eq!(session.token, "token-value");
    }

    #[test]
    fn splits_long_unicode_text_without_breaking_utf8() {
        let text = "你".repeat(901);
        let chunks = split_chunks(&text);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), 900);
        assert_eq!(chunks[1], "你");
    }
}
