use std::{collections::BTreeMap, io::Read, sync::Arc, time::Duration};

use crate::error::{AppleHmeError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
}

#[derive(Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, Vec<String>>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .and_then(|(_, values)| values.first())
            .map(String::as_str)
    }

    pub fn headers<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.headers
            .iter()
            .filter(move |(key, _)| key.eq_ignore_ascii_case(name))
            .flat_map(|(_, values)| values.iter().map(String::as_str))
    }
}

pub trait HttpTransport: Send + Sync + 'static {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse>;
}

impl<T: HttpTransport + ?Sized> HttpTransport for Arc<T> {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        (**self).execute(request)
    }
}

#[derive(Clone)]
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

impl UreqTransport {
    pub fn new(timeout: Duration) -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(timeout)
                .redirects(0)
                .build(),
        }
    }
}

impl HttpTransport for UreqTransport {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        let mut builder = match request.method {
            HttpMethod::Get => self.agent.get(&request.url),
            HttpMethod::Post => self.agent.post(&request.url),
            HttpMethod::Put => self.agent.put(&request.url),
        };
        for (name, value) in request.headers {
            builder = builder.set(&name, &value);
        }
        let response = match request.body {
            Some(body) => builder.send_bytes(&body),
            None => builder.call(),
        };
        let response = match response {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            Err(error) => {
                return Err(AppleHmeError::network(format!(
                    "Apple HTTP 请求失败：{error}"
                )))
            }
        };
        let status = response.status();
        let mut headers = BTreeMap::new();
        for name in response.headers_names() {
            headers.insert(
                name.clone(),
                response
                    .all(&name)
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            );
        }
        let mut reader = response.into_reader().take(4 << 20);
        let mut body = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut body).map_err(|error| {
            AppleHmeError::network(format!("读取 Apple HTTP 响应失败：{error}"))
        })?;
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}
