use std::{
    collections::HashSet,
    io::Read,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    time::Duration,
};

use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use imail_core::{contacts::contact_domain, LogoFetchAttemptRecord};
use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

const MAX_HTML_BYTES: usize = 512 * 1024;
pub(crate) const MAX_IMAGE_BYTES: usize = 1024 * 1024;
const FAILURE_TTL: ChronoDuration = ChronoDuration::hours(24);
pub(crate) const NEGATIVE_CACHE_VERSION: u8 = 3;

#[derive(Debug, Clone)]
pub struct LogoSource {
    pub address: String,
    pub name: String,
    pub html: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct DiscoveredLogo {
    pub content: Vec<u8>,
    pub content_type: String,
    pub source_url: String,
    pub fetched_at: String,
    pub key: String,
}

#[derive(Debug, Clone)]
pub struct LogoAttempt {
    pub target: String,
    pub domain_key: String,
    pub status: String,
    pub detail: String,
    pub attempted_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct LogoDiscoveryReport {
    pub result: Option<DiscoveredLogo>,
    pub permanent_failure: bool,
    pub attempts: Vec<LogoAttempt>,
}

pub trait LogoDiscoveryPort: Send + Sync {
    fn discover(
        &self,
        source: &LogoSource,
        previous: &[LogoFetchAttemptRecord],
    ) -> LogoDiscoveryReport;
}

#[derive(Debug, Default)]
pub struct NetworkLogoDiscovery;

impl LogoDiscoveryPort for NetworkLogoDiscovery {
    fn discover(
        &self,
        source: &LogoSource,
        previous: &[LogoFetchAttemptRecord],
    ) -> LogoDiscoveryReport {
        discover(source, previous)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CachedLogoMeta {
    pub content_type: String,
    pub source_url: String,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MissingLogoMeta {
    pub unavailable_at: String,
    pub version: u8,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub permanent: bool,
}

fn discover(source: &LogoSource, previous: &[LogoFetchAttemptRecord]) -> LogoDiscoveryReport {
    let domain_key = sender_key(source);
    let now = Utc::now();
    let mut attempted = previous
        .iter()
        .filter(|attempt| {
            attempt.status == "success"
                || (attempt.status == "failed"
                    && fresh_failure(&attempt.attempted_at, now)
                    && attempt.detail != "远程请求失败：403")
        })
        .map(|attempt| attempt.target.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut report = LogoDiscoveryReport::default();
    for site in site_candidates(source) {
        let target = site.origin().ascii_serialization().to_ascii_lowercase();
        if attempted.contains(&target) {
            continue;
        }
        let mut detail = "未找到有效的网站图标".to_string();
        let mut icons = vec![site.join("/favicon.ico").expect("absolute site URL")];
        match safe_fetch(
            site.clone(),
            "text/html,application/xhtml+xml",
            MAX_HTML_BYTES,
        ) {
            Ok(page) => {
                if !page.content_type.contains("text/html")
                    && !page.content_type.contains("application/xhtml+xml")
                {
                    detail = "网站未返回 HTML".into();
                } else if let Ok(html) = String::from_utf8(page.content) {
                    if is_challenge_page(&html) {
                        detail = "网站返回了访问验证页".into();
                    } else {
                        icons = discover_icons(&html, &page.final_url);
                    }
                } else {
                    detail = "网站未返回有效 UTF-8 HTML".into();
                }
            }
            Err(error) => {
                detail = error.detail.clone();
                if error.permanent {
                    report.permanent_failure = true;
                    report
                        .attempts
                        .push(attempt(&target, &domain_key, "failed", detail));
                    return report;
                }
            }
        }
        for icon in icons {
            match safe_fetch(
                icon,
                "image/png,image/jpeg,image/webp,image/gif,image/x-icon",
                MAX_IMAGE_BYTES,
            ) {
                Ok(image) => {
                    if let Some(kind) = image_type(&image.content) {
                        let key = format!(
                            "domain:{}",
                            site.host_str().unwrap_or_default().to_ascii_lowercase()
                        );
                        let fetched_at = timestamp();
                        report.attempts.push(attempt(
                            &target,
                            &domain_key,
                            "success",
                            image.final_url.as_str().to_string(),
                        ));
                        report.result = Some(DiscoveredLogo {
                            content: image.content,
                            content_type: kind.into(),
                            source_url: image.final_url.into(),
                            fetched_at,
                            key,
                        });
                        return report;
                    }
                    detail = "图标内容不是受支持的图片格式".into();
                }
                Err(error) => {
                    detail = error.detail.clone();
                    if error.permanent {
                        report.permanent_failure = true;
                        report
                            .attempts
                            .push(attempt(&target, &domain_key, "failed", detail));
                        return report;
                    }
                }
            }
        }
        report
            .attempts
            .push(attempt(&target, &domain_key, "failed", detail));
        attempted.insert(target);
    }
    report
}

fn attempt(target: &str, domain_key: &str, status: &str, detail: String) -> LogoAttempt {
    LogoAttempt {
        target: target.into(),
        domain_key: domain_key.into(),
        status: status.into(),
        detail: sanitize_detail(&detail),
        attempted_at: timestamp(),
    }
}

fn sender_key(source: &LogoSource) -> String {
    contact_domain(
        &source.address,
        &imail_core::contacts::PublicSuffixDomainResolver,
    )
    .map(|domain| format!("domain:{}", domain.hostname))
    .unwrap_or_else(|| {
        let fallback = if source.address.trim().is_empty() {
            source.name.trim()
        } else {
            source.address.trim()
        };
        format!("sender:{}", fallback.to_ascii_lowercase())
    })
}

pub(crate) fn fresh_failure(value: &str, now: DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| now.signed_duration_since(value.with_timezone(&Utc)) < FAILURE_TTL)
        .unwrap_or(false)
}

pub(crate) fn site_candidates(source: &LogoSource) -> Vec<Url> {
    let domain = contact_domain(
        &source.address,
        &imail_core::contacts::PublicSuffixDomainResolver,
    );
    let href = Regex::new(r#"(?i)\bhref\s*=\s*["']([^"']+)["']"#).expect("static regex");
    let text_url = Regex::new(r#"(?i)https?://[^\s<>"']+"#).expect("static regex");
    let excluded =
        Regex::new(r"(?i)unsubscribe|optout|tracking|/track|/click").expect("static regex");
    let mut values = href
        .captures_iter(&source.html)
        .filter_map(|capture| {
            capture
                .get(1)
                .map(|value| value.as_str().replace("&amp;", "&"))
        })
        .chain(
            text_url
                .find_iter(&source.text)
                .map(|value| value.as_str().to_string()),
        )
        .filter_map(|value| Url::parse(&value).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .filter(|url| {
            !excluded.is_match(&format!(
                "{}{}",
                url.host_str().unwrap_or_default(),
                url.path()
            ))
        })
        .filter(|url| {
            domain.as_ref().map_or(true, |sender| {
                url.host_str()
                    .and_then(|host| {
                        contact_domain(
                            &format!("x@{host}"),
                            &imail_core::contacts::PublicSuffixDomainResolver,
                        )
                    })
                    .is_some_and(|candidate| candidate.registrable == sender.registrable)
            })
        })
        .filter_map(|url| Url::parse(&url.origin().ascii_serialization()).ok())
        .collect::<Vec<_>>();
    if let Some(domain) = domain {
        values.extend(
            [
                format!("https://{}", domain.hostname),
                format!("https://{}", domain.registrable),
                format!("https://www.{}", domain.registrable),
            ]
            .into_iter()
            .filter_map(|value| Url::parse(&value).ok()),
        );
    }
    deduplicate_urls(values, 8, |url| url.origin().ascii_serialization())
}

pub(crate) fn discover_icons(html: &str, page: &Url) -> Vec<Url> {
    let tags = Regex::new(r"(?is)<link\b[^>]*>").expect("static regex");
    let rel = Regex::new(r#"(?i)\brel\s*=\s*["']([^"']+)["']"#).expect("static regex");
    let href = Regex::new(r#"(?i)\bhref\s*=\s*["']([^"']+)["']"#).expect("static regex");
    let icon_rel =
        Regex::new(r"(?i)(?:^|\s)(?:apple-touch-icon|icon)(?:\s|$)").expect("static regex");
    let mut values = tags
        .find_iter(html)
        .filter_map(|tag| {
            let rel_value = rel.captures(tag.as_str())?.get(1)?.as_str();
            if !icon_rel.is_match(rel_value) {
                return None;
            }
            let value = href
                .captures(tag.as_str())?
                .get(1)?
                .as_str()
                .replace("&amp;", "&");
            page.join(&value).ok()
        })
        .collect::<Vec<_>>();
    if let Ok(favicon) = page.join("/favicon.ico") {
        values.push(favicon);
    }
    deduplicate_urls(values, 5, |url| url.as_str().to_string())
}

fn deduplicate_urls(values: Vec<Url>, maximum: usize, key: impl Fn(&Url) -> String) -> Vec<Url> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(key(value)))
        .take(maximum)
        .collect()
}

pub(crate) fn image_type(content: &[u8]) -> Option<&'static str> {
    if content.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) {
        Some("image/png")
    } else if content.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if content.starts_with(b"GIF87a") || content.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if content.starts_with(b"RIFF") && content.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if content.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else {
        None
    }
}

pub(crate) fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(value) => {
            let [a, b, ..] = value.octets();
            !(a == 0
                || a == 10
                || a == 127
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 100 && (64..=127).contains(&b))
                || a >= 224)
        }
        IpAddr::V6(value) => {
            if let Some(v4) = value.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let first = value.segments()[0];
            !value.is_unspecified()
                && !value.is_loopback()
                && first & 0xfe00 != 0xfc00
                && first & 0xffc0 != 0xfe80
                && first & 0xff00 != 0xff00
        }
    }
}

struct FetchResponse {
    final_url: Url,
    content_type: String,
    content: Vec<u8>,
}

#[derive(Debug)]
struct FetchError {
    detail: String,
    permanent: bool,
}

fn safe_fetch(mut current: Url, accepts: &str, limit: usize) -> Result<FetchResponse, FetchError> {
    for redirect in 0..=3 {
        validate_url(&current)?;
        let addresses = resolve_public(&current)?;
        let selected = addresses[0];
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(10))
            .resolver(move |_: &str| Ok(vec![selected]))
            .build();
        let response = match agent
            .get(current.as_str())
            .set("Accept", accepts)
            .set("User-Agent", "iMail Logo Fetcher/1.0")
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            Err(error) => return Err(fetch_error(error.to_string(), false)),
        };
        let status = response.status();
        if (300..400).contains(&status) {
            let Some(location) = response.header("location") else {
                return Err(fetch_error("重定向缺少 Location", false));
            };
            if redirect == 3 {
                return Err(fetch_error("重定向过多", false));
            }
            current = current
                .join(location)
                .map_err(|_| fetch_error("重定向网址无效", false))?;
            continue;
        }
        if status == 403
            && (response.header("cf-ray").is_some()
                || response.header("cf-mitigated").is_some()
                || response
                    .header("server")
                    .is_some_and(|value| value.to_ascii_lowercase().contains("cloudflare")))
        {
            return Err(fetch_error("Cloudflare 返回 403，停止重试", true));
        }
        if !(200..300).contains(&status) {
            return Err(fetch_error(format!("远程请求失败：{status}"), false));
        }
        if response
            .header("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|size| size > limit)
        {
            return Err(fetch_error("远程内容过大", false));
        }
        let content_type = response
            .header("content-type")
            .unwrap_or_default()
            .to_string();
        let mut content = Vec::new();
        response
            .into_reader()
            .take((limit + 1) as u64)
            .read_to_end(&mut content)
            .map_err(|error| fetch_error(error.to_string(), false))?;
        if content.len() > limit {
            return Err(fetch_error("远程内容过大", false));
        }
        return Ok(FetchResponse {
            final_url: current,
            content_type,
            content,
        });
    }
    Err(fetch_error("重定向过多", false))
}

fn validate_url(url: &Url) -> Result<(), FetchError> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || url.port().is_some_and(|port| !matches!(port, 80 | 443))
    {
        return Err(fetch_error("不安全的网址", false));
    }
    Ok(())
}

fn resolve_public(url: &Url) -> Result<Vec<SocketAddr>, FetchError> {
    let host = url
        .host_str()
        .ok_or_else(|| fetch_error("不安全的网址", false))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| fetch_error("不允许的端口", false))?;
    let addresses = if let Ok(address) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(address, port)]
    } else {
        (host, port)
            .to_socket_addrs()
            .map_err(|error| fetch_error(error.to_string(), false))?
            .collect()
    };
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(fetch_error("不允许访问内网地址", false));
    }
    Ok(addresses)
}

fn fetch_error(detail: impl Into<String>, permanent: bool) -> FetchError {
    FetchError {
        detail: sanitize_detail(&detail.into()),
        permanent,
    }
}

fn sanitize_detail(value: &str) -> String {
    value.replace(['\r', '\n'], " ").chars().take(300).collect()
}

fn is_challenge_page(html: &str) -> bool {
    Regex::new(r"(?i)(?:<title>\s*(?:just a moment|attention required[^<]*cloudflare)|cdn-cgi/challenge-platform|cf-browser-verification|\bcf-chl-)")
        .expect("static regex")
        .is_match(html)
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_private_reserved_and_mapped_addresses() {
        for value in [
            "0.1.2.3",
            "10.0.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "172.31.0.1",
            "192.168.0.1",
            "100.64.0.1",
            "224.0.0.1",
            "::",
            "::1",
            "fc00::1",
            "fe80::1",
            "ff00::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public_ip(value.parse().unwrap()), "{value}");
        }
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
        assert!(is_public_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn derives_same_domain_sites_and_icons_without_tracking_links() {
        let source = LogoSource {
            address: "hello@mail.example.co.uk".into(),
            name: String::new(),
            html: r#"<a href="https://news.example.co.uk/welcome">ok</a><a href="https://track.example.co.uk/click">bad</a>"#.into(),
            text: "https://other.invalid/no".into(),
        };
        let sites = site_candidates(&source)
            .into_iter()
            .map(Into::<String>::into)
            .collect::<Vec<_>>();
        assert!(sites
            .iter()
            .any(|value| value == "https://news.example.co.uk/"));
        assert!(!sites.iter().any(|value| value.contains("track")));
        assert!(!sites.iter().any(|value| value.contains("other.invalid")));

        let page = Url::parse("https://example.co.uk/path").unwrap();
        let icons = discover_icons(
            r#"<link rel="stylesheet" href="/no.css"><link href="/icon.png" rel="apple-touch-icon">"#,
            &page,
        );
        assert_eq!(icons[0].as_str(), "https://example.co.uk/icon.png");
        assert_eq!(icons[1].as_str(), "https://example.co.uk/favicon.ico");
    }

    #[test]
    fn detects_only_supported_image_magic_and_bounds_failure_age() {
        assert_eq!(
            image_type(&[137, 80, 78, 71, 13, 10, 26, 10]),
            Some("image/png")
        );
        assert_eq!(image_type(b"<svg></svg>"), None);
        let now = Utc::now();
        assert!(fresh_failure(
            &(now - ChronoDuration::hours(23)).to_rfc3339(),
            now
        ));
        assert!(!fresh_failure(
            &(now - ChronoDuration::hours(25)).to_rfc3339(),
            now
        ));
    }
}
