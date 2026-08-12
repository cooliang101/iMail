use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{
        header::{COOKIE, HOST, ORIGIN, RETRY_AFTER, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use imail_core::authentication::{
    AuthenticationError, AuthenticationService, LoginInput, RegistrationInput,
};
use imail_storage_sqlite::{AuthStoreError, SqliteAuthStore};
use serde::Serialize;
use tokio::task::JoinError;
use url::Url;

use crate::AppState;

const SESSION_COOKIE: &str = "imail_session";

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/auth/status", get(status))
        .route("/api/auth/session", get(session))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
}

#[derive(Clone)]
pub(crate) struct AuthenticatedUser {
    pub user_id: String,
    pub actor: String,
}

#[derive(Serialize)]
struct UserBody<T> {
    user: T,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

enum AuthHttpError {
    Application(AuthenticationError<AuthStoreError>),
    Join,
}

impl From<AuthenticationError<AuthStoreError>> for AuthHttpError {
    fn from(error: AuthenticationError<AuthStoreError>) -> Self {
        Self::Application(error)
    }
}

impl From<JoinError> for AuthHttpError {
    fn from(_: JoinError) -> Self {
        Self::Join
    }
}

pub(crate) async fn require_session(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(raw_session) = session_cookie(request.headers()) else {
        return error(StatusCode::UNAUTHORIZED, "登录已过期，请重新登录");
    };
    match with_service(database_path(&state), move |service| {
        service.session(&raw_session)
    })
    .await
    {
        Ok(user) => {
            let actor = actor(
                request
                    .extensions()
                    .get::<ConnectInfo<SocketAddr>>()
                    .cloned(),
                request.headers(),
                &state,
            );
            request.extensions_mut().insert(AuthenticatedUser {
                user_id: user.id,
                actor,
            });
            next.run(request).await
        }
        Err(AuthHttpError::Application(AuthenticationError::Domain { status: 401, .. })) => {
            error(StatusCode::UNAUTHORIZED, "登录已过期，请重新登录")
        }
        Err(cause) => internal_error(cause),
    }
}

async fn status(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let raw_session = session_cookie(&headers);
    let registration_configured = state.config.registration_open;
    match with_service(database_path(&state), move |service| {
        service.status(raw_session.as_deref(), registration_configured)
    })
    .await
    {
        Ok(body) => Json(body).into_response(),
        Err(cause) => internal_error(cause),
    }
}

async fn session(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(raw_session) = session_cookie(&headers) else {
        return error(StatusCode::UNAUTHORIZED, "请先登录");
    };
    match with_service(database_path(&state), move |service| {
        service.session(&raw_session)
    })
    .await
    {
        Ok(user) => Json(UserBody { user }).into_response(),
        Err(AuthHttpError::Application(AuthenticationError::Domain {
            status, message, ..
        })) if status < 500 => error(status_code(status), message),
        Err(cause) => internal_error(cause),
    }
}

async fn register(
    State(state): State<Arc<AppState>>,
    connect: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    input: Result<Json<RegistrationInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let actor = actor(connect, &headers, &state);
    let registration_configured = state.config.registration_open;
    let input = input.ok().map(|Json(input)| input);
    match with_service(database_path(&state), move |service| {
        service.register(input, &actor, registration_configured)
    })
    .await
    {
        Ok(authenticated) => with_session_cookie(
            (
                StatusCode::CREATED,
                Json(UserBody {
                    user: authenticated.user,
                }),
            )
                .into_response(),
            &headers,
            &authenticated.raw_session,
            &state,
            false,
        ),
        Err(AuthHttpError::Application(AuthenticationError::Domain {
            status, message, ..
        })) => error(status_code(status), message),
        Err(AuthHttpError::Application(AuthenticationError::Limited { retry_after })) => {
            limited(retry_after)
        }
        Err(AuthHttpError::Application(AuthenticationError::Repository(storage_error)))
            if storage_error.is_unique_violation() =>
        {
            error(StatusCode::CONFLICT, "这个登录名已存在")
        }
        Err(cause) => internal_error(cause),
    }
}

async fn login(
    State(state): State<Arc<AppState>>,
    connect: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    input: Result<Json<LoginInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let actor = actor(connect, &headers, &state);
    let input = input.ok().map(|Json(input)| input);
    match with_service(database_path(&state), move |service| {
        service.login(input, &actor)
    })
    .await
    {
        Ok(authenticated) => with_session_cookie(
            Json(UserBody {
                user: authenticated.user,
            })
            .into_response(),
            &headers,
            &authenticated.raw_session,
            &state,
            false,
        ),
        Err(AuthHttpError::Application(AuthenticationError::Domain {
            status, message, ..
        })) => error(status_code(status), message),
        Err(AuthHttpError::Application(AuthenticationError::Limited { retry_after })) => {
            limited(retry_after)
        }
        Err(cause) => internal_error(cause),
    }
}

async fn logout(
    State(state): State<Arc<AppState>>,
    connect: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
) -> Response {
    let raw_session = session_cookie(&headers);
    let actor = actor(connect, &headers, &state);
    match with_service(database_path(&state), move |service| {
        service.logout(raw_session.as_deref(), &actor)
    })
    .await
    {
        Ok(()) => with_session_cookie(
            StatusCode::NO_CONTENT.into_response(),
            &headers,
            "",
            &state,
            true,
        ),
        Err(cause) => internal_error(cause),
    }
}

fn actor(
    connect: Option<ConnectInfo<SocketAddr>>,
    headers: &HeaderMap,
    state: &AppState,
) -> String {
    if state.config.trust_proxy_one_hop {
        if let Some(address) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit(',').next())
            .map(str::trim)
            .filter(|value| value.parse::<std::net::IpAddr>().is_ok())
        {
            return address.to_string();
        }
    }
    connect
        .map(|ConnectInfo(address)| address.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn database_path(state: &AppState) -> PathBuf {
    state.config.data_dir.join("imail.sqlite")
}

async fn with_service<T, F>(database: PathBuf, operation: F) -> Result<T, AuthHttpError>
where
    T: Send + 'static,
    F: FnOnce(
            &mut AuthenticationService<'_, SqliteAuthStore>,
        ) -> Result<T, AuthenticationError<AuthStoreError>>
        + Send
        + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut store =
            SqliteAuthStore::open_database(database).map_err(AuthenticationError::Repository)?;
        operation(&mut AuthenticationService::new(&mut store))
    })
    .await?
    .map_err(Into::into)
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|value| value.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty() && value.len() <= 256)
}

fn with_session_cookie(
    mut response: Response,
    request_headers: &HeaderMap,
    value: &str,
    state: &AppState,
    clear: bool,
) -> Response {
    let cross_origin = request_is_cross_origin(request_headers);
    let same_site = if cross_origin { "None" } else { "Lax" };
    let forwarded_https = state.config.trust_proxy_one_hop
        && request_headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .is_some_and(|value| value.trim() == "https");
    let secure = state.config.secure_cookies || cross_origin || forwarded_https;
    let maximum_age = if clear { 0 } else { 2_592_000 };
    let cookie = format!(
        "{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite={same_site}; Max-Age={maximum_age}{}",
        if secure { "; Secure" } else { "" }
    );
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
}

fn request_is_cross_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Url::parse(value).ok())
    else {
        return false;
    };
    let Some(host) = headers.get(HOST).and_then(|value| value.to_str().ok()) else {
        return true;
    };
    origin
        .host_str()
        .map(|origin_host| {
            let authority = host
                .parse::<axum::http::uri::Authority>()
                .ok()
                .map(|authority| {
                    (
                        authority
                            .host()
                            .trim_start_matches('[')
                            .trim_end_matches(']')
                            .to_ascii_lowercase(),
                        authority.port_u16(),
                    )
                });
            authority
                .map(|(request_host, request_port)| {
                    let origin_port = origin.port_or_known_default();
                    let request_port = request_port.or(origin_port);
                    request_host != origin_host.to_ascii_lowercase() || request_port != origin_port
                })
                .unwrap_or(true)
        })
        .unwrap_or(true)
}

fn limited(retry_after: u64) -> Response {
    let mut response = error(StatusCode::TOO_MANY_REQUESTS, "尝试过多，请稍后再试");
    if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
        response.headers_mut().insert(RETRY_AFTER, value);
    }
    response
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(ErrorBody {
            error: message.into(),
        }),
    )
        .into_response()
}

fn internal_error(cause: AuthHttpError) -> Response {
    match cause {
        AuthHttpError::Application(AuthenticationError::Repository(error)) => {
            eprintln!("[imail-http] auth storage error: {error}");
        }
        AuthHttpError::Application(error) => {
            eprintln!("[imail-http] unexpected auth application error: {error}");
        }
        AuthHttpError::Join => eprintln!("[imail-http] auth worker stopped unexpectedly"),
    }
    error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求")
}
