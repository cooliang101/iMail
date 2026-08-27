mod azure;
mod deepl;
mod google;

use std::{thread, time::Duration};

use serde::Serialize;

pub(crate) use azure::{AzureClient, AzureTranslationRequest};
pub(crate) use deepl::{DeepLClient, DeepLTranslationRequest};
pub(crate) use google::{GoogleClient, GoogleCredential, GoogleTranslationRequest};

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
    #[error("翻译服务配置无效")]
    InvalidConfiguration,
}

impl ProviderExecutionError {
    pub(crate) const fn status(&self) -> u16 {
        match self {
            Self::Authentication => 403,
            Self::QuotaExceeded | Self::RateLimited => 429,
            Self::UnsupportedLanguage | Self::InvalidConfiguration => 400,
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
            Self::InvalidConfiguration => "TRANSLATION_PROVIDER_CONFIG_INVALID",
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
            Self::InvalidConfiguration => "翻译服务配置无效",
        }
    }
}

fn send_json_with_retry(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
    body: &impl Serialize,
) -> Result<ureq::Response, ureq::Error> {
    let body = serde_json::to_string(body).expect("provider request body serializes");
    let delays = [Duration::from_millis(250), Duration::from_millis(750)];
    for attempt in 0..=delays.len() {
        let mut request = agent
            .post(url)
            .set("Content-Type", "application/json; charset=utf-8");
        for (name, value) in headers {
            request = request.set(name, value);
        }
        match request.send_string(&body) {
            Err(error @ ureq::Error::Status(status, _)) if status == 429 || status >= 500 => {
                if let Some(delay) = delays.get(attempt) {
                    thread::sleep(*delay);
                    continue;
                }
                return Err(error);
            }
            result => return result,
        }
    }
    unreachable!("retry loop always returns")
}
