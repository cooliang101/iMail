use std::collections::BTreeMap;

use serde::{de::DeserializeOwned, Serialize};
use url::Url;

use crate::{
    error::{AppleHmeError, Result},
    model::AppleCookie,
    transport::{HttpMethod, HttpRequest, HttpResponse, HttpTransport},
};

pub(crate) const ICLOUD_WEB_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3.1 Safari/605.1.15";
pub(crate) const APPLE_ACCOUNT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";

pub(crate) fn execute<T: HttpTransport>(
    transport: &T,
    method: HttpMethod,
    url: String,
    mut headers: BTreeMap<String, String>,
    body: Option<Vec<u8>>,
    cookies: &mut Vec<AppleCookie>,
) -> Result<HttpResponse> {
    if !cookies.is_empty() {
        if let Some(value) = cookie_header(cookies, &url) {
            headers.insert("Cookie".into(), value);
        }
    }
    let response = transport.execute(HttpRequest {
        method,
        url: url.clone(),
        headers,
        body,
    })?;
    merge_response_cookies(cookies, &url, &response);
    Ok(response)
}

pub(crate) fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| AppleHmeError::invalid("Apple 请求参数无法序列化"))
}

pub(crate) fn decode_json<T: DeserializeOwned>(response: &HttpResponse) -> Result<T> {
    serde_json::from_slice(&response.body)
        .map_err(|_| AppleHmeError::bad_response("Apple 返回了无法解析的 JSON"))
}

pub(crate) fn response_text(response: &HttpResponse) -> String {
    let value = String::from_utf8_lossy(&response.body);
    value.chars().take(600).collect()
}

pub(crate) fn header_map(entries: &[(&str, String)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect()
}

pub(crate) fn cookie_header(cookies: &[AppleCookie], raw_url: &str) -> Option<String> {
    let url = Url::parse(raw_url).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let path = if url.path().is_empty() {
        "/"
    } else {
        url.path()
    };
    let now = chrono::Utc::now().timestamp();
    let values = cookies
        .iter()
        .filter(|cookie| {
            !cookie.name.is_empty()
                && !cookie.value.is_empty()
                && cookie.expires_at.map_or(true, |expires| expires > now)
                && (!cookie.secure || url.scheme() == "https")
                && domain_matches(&host, &cookie.domain, &cookie.name)
                && path.starts_with(if cookie.path.is_empty() {
                    "/"
                } else {
                    &cookie.path
                })
        })
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join("; "))
}

fn merge_response_cookies(
    cookies: &mut Vec<AppleCookie>,
    request_url: &str,
    response: &HttpResponse,
) {
    let Ok(url) = Url::parse(request_url) else {
        return;
    };
    for raw in response.headers("set-cookie") {
        let Some(next) = parse_set_cookie(raw, &url) else {
            continue;
        };
        let position = cookies.iter().position(|current| {
            current.name == next.name
                && current.domain.eq_ignore_ascii_case(&next.domain)
                && current.path == next.path
        });
        let deleted = next.value.is_empty() || next.expires_at.is_some_and(|value| value <= 0);
        match (position, deleted) {
            (Some(index), true) => {
                cookies.remove(index);
            }
            (Some(index), false) => cookies[index] = next,
            (None, false) => cookies.push(next),
            (None, true) => {}
        }
    }
}

fn parse_set_cookie(raw: &str, request_url: &Url) -> Option<AppleCookie> {
    let mut parts = raw.split(';');
    let (name, value) = parts.next()?.split_once('=')?;
    let mut cookie = AppleCookie {
        name: name.trim().to_string(),
        value: value.trim().to_string(),
        domain: request_url.host_str()?.to_ascii_lowercase(),
        path: "/".into(),
        expires_at: None,
        secure: false,
        http_only: false,
    };
    for attribute in parts {
        let attribute = attribute.trim();
        let (name, value) = attribute
            .split_once('=')
            .map_or((attribute, ""), |(name, value)| (name.trim(), value.trim()));
        match name.to_ascii_lowercase().as_str() {
            "domain" if !value.is_empty() => cookie.domain = value.to_ascii_lowercase(),
            "path" if !value.is_empty() => cookie.path = value.to_string(),
            "max-age" => {
                if let Ok(seconds) = value.parse::<i64>() {
                    cookie.expires_at = Some(if seconds <= 0 {
                        0
                    } else {
                        chrono::Utc::now().timestamp() + seconds
                    });
                }
            }
            "secure" => cookie.secure = true,
            "httponly" => cookie.http_only = true,
            _ => {}
        }
    }
    (!cookie.name.is_empty()).then_some(cookie)
}

fn domain_matches(host: &str, domain: &str, cookie_name: &str) -> bool {
    let domain = domain.trim().trim_start_matches('.').to_ascii_lowercase();
    host == domain
        || host.ends_with(&format!(".{domain}"))
        || (cross_domain_icloud_cookie(host) && is_icloud_web_cookie(cookie_name))
}

fn is_icloud_web_cookie(name: &str) -> bool {
    name.starts_with("X_APPLE_WEB_KB-")
        || matches!(
            name,
            "X-APPLE-UNIQUE-CLIENT-ID"
                | "X-APPLE-WEBAUTH-USER"
                | "X-Apple-GCBD-Cookie"
                | "X-APPLE-WEBAUTH-HSA-TRUST"
                | "X-APPLE-WEBAUTH-PCS-Documents"
                | "X-APPLE-WEBAUTH-PCS-Photos"
                | "X-APPLE-WEBAUTH-PCS-Cloudkit"
                | "X-APPLE-WEBAUTH-PCS-Safari"
                | "X-APPLE-WEBAUTH-PCS-Mail"
                | "X-APPLE-WEBAUTH-LOGIN"
                | "X-APPLE-DS-WEB-SESSION-TOKEN"
                | "X-APPLE-WEB-ID"
                | "X-APPLE-WEBAUTH-VALIDATE"
                | "X-APPLE-WEBAUTH-TOKEN"
        )
}

fn cross_domain_icloud_cookie(host: &str) -> bool {
    (host == "icloud.com"
        || host.ends_with(".icloud.com")
        || host == "icloud.com.cn"
        || host.ends_with(".icloud.com.cn"))
        && (host.contains("maildomainws")
            || host.contains("premiummailsettings")
            || host.contains("mccgateway"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_and_rotates_cookies() {
        let mut cookies = vec![AppleCookie {
            name: "session".into(),
            value: "old".into(),
            domain: ".icloud.com".into(),
            path: "/".into(),
            expires_at: None,
            secure: true,
            http_only: true,
        }];
        assert_eq!(
            cookie_header(&cookies, "https://p1-maildomainws.icloud.com/v1/hme").as_deref(),
            Some("session=old")
        );
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::from([(
                "Set-Cookie".into(),
                vec!["session=new; Domain=.icloud.com; Path=/; Secure; HttpOnly".into()],
            )]),
            body: Vec::new(),
        };
        merge_response_cookies(&mut cookies, "https://www.icloud.com/", &response);
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].value, "new");
    }
}
