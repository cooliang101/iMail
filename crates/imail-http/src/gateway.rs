use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Path, Query, Request, State,
    },
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, SecondsFormat, Utc};
use futures_util::StreamExt;
use imail_core::{
    external_access::ExternalAccessService,
    mail_operations::MailApplicationError,
    messages::{GatewayMessageCursor, GatewayMessageQuery},
    AccountRepository, DeveloperTokenRepository, MessageRepository,
};
use imail_protocol::{DeveloperTokenReadModel, MessageReadModel, SendMessageInput};
use imail_storage_sqlite::SyncRuntimeStore;
use imail_storage_sqlite::{AuthStoreError, SqliteAuthStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::{messages, AppState};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health))
        .route("/mailboxes", get(mailboxes))
        .route("/messages", get(list_messages))
        .route("/mailboxes/:mailbox/messages", get(list_mailbox_messages))
        .route("/messages/:message_id", get(message_detail))
        .route(
            "/messages/:message_id/attachments/:index",
            get(download_attachment),
        )
        .route("/send", post(send))
        .route("/events", get(events_websocket))
        .fallback(not_found)
        .layer(axum::middleware::from_fn(request_context))
}

pub(crate) fn docs_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/openapi.json", get(openapi))
        .route("/docs", get(docs))
        .route("/docs/", get(docs))
}

#[derive(Clone)]
struct RequestId(String);

async fn request_context(mut request: Request, next: Next) -> Response {
    let supplied = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|value| value.is_ascii_alphanumeric() || b"._:-".contains(&value))
        })
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    request.extensions_mut().insert(RequestId(supplied.clone()));
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&supplied) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn health() -> Json<Value> {
    Json(json!({ "service": "imail-gateway", "version": "v1", "ok": true }))
}

async fn mailboxes(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Response {
    let token = match authenticate(&state, &headers, "accounts:read").await {
        Ok(token) => token,
        Err(error) => return gateway_error(&request_id.0, error),
    };
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        let allowed = token.account_ids.into_iter().collect::<HashSet<_>>();
        Ok(store
            .list_accounts(&token.owner_id)?
            .into_iter()
            .filter(|account| allowed.contains(&account.id))
            .map(|account| {
                json!({
                    "email": account.email,
                    "provider": account.provider,
                    "displayName": account.display_name,
                    "group": account.group,
                    "status": account.status,
                    "lastSyncAt": account.last_sync_at,
                })
            })
            .collect::<Vec<_>>())
    })
    .await
    {
        Ok(mailboxes) => Json(json!({ "mailboxes": mailboxes })).into_response(),
        Err(_) => gateway_error(&request_id.0, GatewayFailure::internal()),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageQueryInput {
    mailbox: Option<String>,
    limit: Option<String>,
    cursor: Option<String>,
    mailbox_role: Option<String>,
    unread: Option<String>,
    since: Option<String>,
    before: Option<String>,
    q: Option<String>,
}

async fn list_messages(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    query: Result<Query<MessageQueryInput>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return gateway_error(&request_id.0, GatewayFailure::invalid()),
    };
    list_messages_inner(state, request_id, headers, query).await
}

async fn list_mailbox_messages(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(mailbox): Path<String>,
    query: Result<Query<MessageQueryInput>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(mut query) = match query {
        Ok(query) => query,
        Err(_) => return gateway_error(&request_id.0, GatewayFailure::invalid()),
    };
    query.mailbox = Some(mailbox);
    list_messages_inner(state, request_id, headers, query).await
}

async fn list_messages_inner(
    state: Arc<AppState>,
    request_id: RequestId,
    headers: HeaderMap,
    input: MessageQueryInput,
) -> Response {
    let token = match authenticate(&state, &headers, "messages:read").await {
        Ok(token) => token,
        Err(error) => return gateway_error(&request_id.0, error),
    };
    let input = match validate_query(input) {
        Ok(input) => input,
        Err(error) => return gateway_error(&request_id.0, error),
    };
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        let accounts = store.list_accounts(&token.owner_id)?;
        let permitted = token.account_ids.iter().cloned().collect::<HashSet<_>>();
        let (account_ids, recipient, public_mailbox) = if let Some(mailbox) = &input.mailbox {
            if let Some(account) = accounts.iter().find(|account| {
                account.email.eq_ignore_ascii_case(mailbox) && permitted.contains(&account.id)
            }) {
                (vec![account.id.clone()], None, None)
            } else {
                let mut resolved = None;
                for account in &accounts {
                    if !account.provider.eq_ignore_ascii_case("icloud")
                        || !permitted.contains(&account.id)
                    {
                        continue;
                    }
                    let addresses = store.apple_hme_addresses(&token.owner_id, &account.id)?;
                    if let Some(address) = addresses
                        .addresses
                        .into_iter()
                        .find(|address| address.email.eq_ignore_ascii_case(mailbox))
                    {
                        resolved = Some((account.id.clone(), address.email));
                        break;
                    }
                }
                let (account_id, alias) = resolved.ok_or(GatewayStorageError::Mailbox)?;
                (
                    vec![account_id.clone()],
                    Some(alias.clone()),
                    Some((account_id, alias)),
                )
            }
        } else {
            (token.account_ids.clone(), None, None)
        };
        let page = store.query_gateway_messages(
            &token.owner_id,
            &GatewayMessageQuery {
                account_ids,
                recipient,
                mailbox_role: input.mailbox_role,
                unread: input.unread,
                since: input.since,
                before: input.before,
                text: input.q,
                cursor: input.cursor,
                limit: input.limit,
            },
        )?;
        let mut email_by_id = accounts
            .into_iter()
            .map(|account| (account.id, account.email))
            .collect::<HashMap<_, _>>();
        if let Some((account_id, alias)) = public_mailbox {
            email_by_id.insert(account_id, alias);
        }
        let values = page
            .messages
            .iter()
            .filter_map(|message| {
                email_by_id
                    .get(&message.account_id)
                    .map(|email| message_summary(message, email))
            })
            .collect::<Vec<_>>();
        let next_cursor = if page.has_more {
            page.messages.last().map(encode_cursor).transpose()?
        } else {
            None
        };
        Ok(json!({
            "messages": values,
            "page": {
                "limit": input.limit,
                "count": values.len(),
                "hasMore": page.has_more,
                "nextCursor": next_cursor,
            }
        }))
    })
    .await
    {
        Ok(body) => Json(body).into_response(),
        Err(GatewayStorageError::Mailbox) => gateway_error(
            &request_id.0,
            GatewayFailure::new(404, "MAILBOX_NOT_AVAILABLE", "邮箱不存在或未授权"),
        ),
        Err(_) => gateway_error(&request_id.0, GatewayFailure::internal()),
    }
}

async fn message_detail(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(message_id): Path<String>,
) -> Response {
    if message_id.is_empty() || message_id.chars().count() > 200 {
        return gateway_error(&request_id.0, GatewayFailure::invalid());
    }
    let token = match authenticate(&state, &headers, "messages:read").await {
        Ok(token) => token,
        Err(error) => return gateway_error(&request_id.0, error),
    };
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        let message = store
            .message(&token.owner_id, &message_id)?
            .filter(|message| token.account_ids.contains(&message.account_id))
            .ok_or(GatewayStorageError::Message)?;
        let account = store
            .account(&token.owner_id, &message.account_id)?
            .ok_or(GatewayStorageError::Message)?;
        let public_mailbox = if account.provider.eq_ignore_ascii_case("icloud") {
            store
                .apple_hme_addresses(&token.owner_id, &account.id)?
                .addresses
                .into_iter()
                .find(|address| message_has_recipient(&message.to, &address.email))
                .map(|address| address.email)
                .unwrap_or(account.email)
        } else {
            account.email
        };
        let mut value = message_summary(&message, &public_mailbox);
        if let Some(object) = value.as_object_mut() {
            object.insert("text".into(), Value::String(message.text));
            if let Some(html) = message.html.filter(|value| !value.is_empty()) {
                object.insert("html".into(), Value::String(html));
            }
        }
        Ok(value)
    })
    .await
    {
        Ok(message) => Json(json!({ "message": message })).into_response(),
        Err(GatewayStorageError::Message) => gateway_error(
            &request_id.0,
            GatewayFailure::new(404, "MESSAGE_NOT_FOUND", "邮件不存在或无权访问"),
        ),
        Err(_) => gateway_error(&request_id.0, GatewayFailure::internal()),
    }
}

async fn download_attachment(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path((message_id, index)): Path<(String, String)>,
) -> Response {
    let Some(index) = index.parse::<usize>().ok() else {
        return gateway_error(&request_id.0, GatewayFailure::invalid());
    };
    let token = match authenticate(&state, &headers, "messages:read").await {
        Ok(token) => token,
        Err(error) => return gateway_error(&request_id.0, error),
    };
    let database = state.config.data_dir.join("imail.sqlite");
    let owner = token.owner_id.clone();
    let authorized = run(database, move |store| {
        let message = store
            .message(&token.owner_id, &message_id)?
            .filter(|message| token.account_ids.contains(&message.account_id))
            .ok_or(GatewayStorageError::Message)?;
        if message
            .attachments
            .as_array()
            .is_some_and(|items| items.get(index).is_some())
        {
            Ok(message.id)
        } else {
            Err(GatewayStorageError::Attachment)
        }
    })
    .await;
    let message_id = match authorized {
        Ok(message_id) => message_id,
        Err(GatewayStorageError::Message | GatewayStorageError::Attachment) => {
            return gateway_error(
                &request_id.0,
                GatewayFailure::new(404, "ATTACHMENT_NOT_FOUND", "附件不存在或无权访问"),
            )
        }
        Err(_) => return gateway_error(&request_id.0, GatewayFailure::internal()),
    };
    match messages::download_attachment_for(state, owner, message_id, index).await {
        Ok(attachment) => {
            let filename = attachment.filename.replace(['\r', '\n', '"', '\\'], "_");
            let encoded: String =
                url::form_urlencoded::byte_serialize(filename.as_bytes()).collect();
            let content_type = HeaderValue::from_str(if attachment.content_type.is_empty() {
                "application/octet-stream"
            } else {
                &attachment.content_type
            })
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
            let disposition =
                HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{encoded}"))
                    .unwrap_or_else(|_| HeaderValue::from_static("attachment"));
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CONTENT_DISPOSITION, disposition),
                ],
                attachment.content,
            )
                .into_response()
        }
        Err(error) => gateway_mail_error(&request_id.0, error),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SendInput {
    #[serde(default)]
    bcc: Vec<String>,
    #[serde(default)]
    in_reply_to: Vec<String>,
    #[serde(default)]
    references: Vec<String>,
    mailbox: String,
    to: Vec<String>,
    cc: Option<Vec<String>>,
    subject: String,
    text: String,
    html: Option<String>,
}

impl SendInput {
    fn envelope(&self) -> imail_protocol::ComposeEnvelope {
        imail_protocol::ComposeEnvelope {
            bcc: self.bcc.clone(),
            reply: imail_protocol::ReplyHeaders {
                in_reply_to: self.in_reply_to.clone(),
                references: self.references.clone(),
            },
        }
    }
}

async fn send(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    input: Result<Json<SendInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) if valid_send(&input) => input,
        _ => return gateway_error(&request_id.0, GatewayFailure::invalid()),
    };
    let token = match authenticate(&state, &headers, "messages:send").await {
        Ok(token) => token,
        Err(error) => return gateway_error(&request_id.0, error),
    };
    let database = state.config.data_dir.join("imail.sqlite");
    let mailbox = input.mailbox.clone();
    let owner = token.owner_id.clone();
    let account_id = match run(database, move |store| {
        store
            .list_accounts(&token.owner_id)?
            .into_iter()
            .find(|account| {
                account.email.eq_ignore_ascii_case(&mailbox)
                    && token.account_ids.contains(&account.id)
            })
            .map(|account| account.id)
            .ok_or(GatewayStorageError::Mailbox)
    })
    .await
    {
        Ok(account_id) => account_id,
        Err(GatewayStorageError::Mailbox) => {
            return gateway_error(
                &request_id.0,
                GatewayFailure::new(404, "MAILBOX_NOT_AVAILABLE", "邮箱不存在或未授权"),
            )
        }
        Err(_) => return gateway_error(&request_id.0, GatewayFailure::internal()),
    };
    let message = SendMessageInput {
        envelope: input.envelope(),
        account_id,
        to: input.to,
        cc: input.cc,
        subject: input.subject.trim().into(),
        text: input.text,
        html: input.html,
        attachments: None,
    };
    match messages::send_for(state, owner, message, None).await {
        Ok(delivery) => {
            (StatusCode::CREATED, Json(json!({ "delivery": delivery }))).into_response()
        }
        Err(error) => gateway_mail_error(&request_id.0, error),
    }
}

async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
    scope: &'static str,
) -> Result<DeveloperTokenReadModel, GatewayFailure> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .map(strip_bearer)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(GatewayFailure::unauthorized)?;
    authenticate_raw(state, raw, scope).await
}

async fn authenticate_raw(
    state: &AppState,
    raw: String,
    scope: &'static str,
) -> Result<DeveloperTokenReadModel, GatewayFailure> {
    let database = state.config.data_dir.join("imail.sqlite");
    run(database, move |store| {
        let token = store
            .authenticate_developer_token(&raw, scope)?
            .filter(|token| !token.owner_id.is_empty() && token.owner_id != "__legacy__")
            .ok_or(GatewayStorageError::Unauthorized)?;
        let enabled = ExternalAccessService::new(store).get(&token.owner_id)?;
        if !enabled.gateway_enabled {
            return Err(GatewayStorageError::Disabled);
        }
        Ok(token)
    })
    .await
    .map_err(|error| match error {
        GatewayStorageError::Unauthorized => GatewayFailure::unauthorized(),
        GatewayStorageError::Disabled => {
            GatewayFailure::new(403, "GATEWAY_DISABLED", "本地网关尚未在 iMail 界面中启用")
        }
        _ => GatewayFailure::internal(),
    })
}

async fn events_websocket(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Response {
    if !websocket_origin_allowed(&state, &headers) {
        return gateway_error(
            &request_id.0,
            GatewayFailure::new(403, "ORIGIN_NOT_ALLOWED", "WebSocket Origin 不受信任"),
        );
    }
    websocket.on_upgrade(move |socket| websocket_session(socket, state, headers))
}

fn websocket_origin_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    let Ok(normalized) = crate::normalize_origin(origin) else {
        return false;
    };
    let Ok(origin_url) = url::Url::parse(&normalized) else {
        return false;
    };
    let same_authority = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|host| url::Url::parse(&format!("{}://{host}", origin_url.scheme())).ok())
        .is_some_and(|host_url| host_url.origin() == origin_url.origin());
    if same_authority {
        return !state.config.production
            || origin_url.scheme() == "https"
            || origin_url.host_str().is_some_and(crate::is_loopback_host);
    }
    state.config.cors_origins.contains(&normalized)
}

async fn websocket_session(mut socket: WebSocket, state: Arc<AppState>, headers: HeaderMap) {
    let mut shutdown = state.shutdown.subscribe();
    let header_token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .map(strip_bearer)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let raw = match header_token {
        Some(raw) => raw,
        None => {
            let first_message = tokio::select! {
                message = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next()) => Some(message),
                _ = shutdown.recv() => None,
            };
            match first_message {
                None => {
                    let _ = socket
                        .send(Message::Close(Some(CloseFrame {
                            code: 1001,
                            reason: "Service shutting down".into(),
                        })))
                        .await;
                    return;
                }
                Some(Ok(Some(Ok(Message::Text(value))))) => {
                    let parsed = serde_json::from_str::<Value>(&value).ok();
                    match parsed
                        .as_ref()
                        .filter(|value| {
                            value.get("type").and_then(Value::as_str) == Some("authenticate")
                        })
                        .and_then(|value| value.get("token"))
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                    {
                        Some(raw) => raw.to_string(),
                        None => {
                            websocket_reject(
                                &mut socket,
                                "INVALID_MESSAGE",
                                "首条消息必须是 authenticate 请求",
                                "Invalid authentication message",
                            )
                            .await;
                            return;
                        }
                    }
                }
                Some(_) => {
                    let _ = socket
                        .send(Message::Close(Some(CloseFrame {
                            code: 1008,
                            reason: "Authentication timeout".into(),
                        })))
                        .await;
                    return;
                }
            }
        }
    };
    if let Err(error) = authenticate_raw(&state, raw.clone(), "messages:read").await {
        websocket_reject(&mut socket, &error.code, &error.message, "Unauthorized").await;
        return;
    }
    let connected = json!({
        "type": "connected",
        "occurredAt": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    });
    if socket
        .send(Message::Text(connected.to_string()))
        .await
        .is_err()
    {
        return;
    }
    let database = state.config.data_dir.join("imail.sqlite");
    let mut cursor = match SyncRuntimeStore::open_database(&database)
        .and_then(|store| store.latest_event_id())
    {
        Ok(cursor) => cursor,
        Err(_) => {
            let _ = socket.close().await;
            return;
        }
    };
    let mut events = tokio::time::interval(std::time::Duration::from_secs(1));
    events.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    events.tick().await;
    heartbeat.tick().await;
    let mut alive = true;
    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                let _ = socket.send(Message::Close(Some(CloseFrame { code: 1001, reason: "Service shutting down".into() }))).await;
                return;
            },
            incoming = socket.next() => match incoming {
                Some(Ok(Message::Pong(_))) => alive = true,
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => {}
            },
            _ = heartbeat.tick() => {
                if !alive {
                    let _ = socket.close().await;
                    return;
                }
                alive = false;
                if socket.send(Message::Ping(Vec::new())).await.is_err() {
                    return;
                }
            },
            _ = events.tick() => {
                let token = match authenticate_raw(&state, raw.clone(), "messages:read").await {
                    Ok(token) => token,
                    Err(_) => {
                        let _ = socket.send(Message::Close(Some(CloseFrame { code: 1008, reason: "Token expired, revoked, or gateway disabled".into() }))).await;
                        return;
                    }
                };
                let store = match SyncRuntimeStore::open_database(&database) {
                    Ok(store) => store,
                    Err(_) => {
                        let _ = socket.send(Message::Close(Some(CloseFrame { code: 1011, reason: "Event delivery failed".into() }))).await;
                        return;
                    }
                };
                let batch = match store.events(cursor, 100) {
                    Ok(batch) => batch,
                    Err(_) => {
                        let _ = socket.send(Message::Close(Some(CloseFrame { code: 1011, reason: "Event delivery failed".into() }))).await;
                        return;
                    }
                };
                for event in batch {
                    cursor = event.id;
                    if event.event_type != "message.created" || !token.account_ids.contains(&event.account_id) {
                        continue;
                    }
                    let value = json!({
                        "id": event.id.to_string(),
                        "type": "message.created",
                        "occurredAt": event.created_at,
                        "data": event.payload,
                    });
                    if socket.send(Message::Text(value.to_string())).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

async fn websocket_reject(socket: &mut WebSocket, code: &str, message: &str, reason: &'static str) {
    let _ = socket
        .send(Message::Text(
            json!({ "type": "error", "error": { "code": code, "message": message } }).to_string(),
        ))
        .await;
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: 1008,
            reason: reason.into(),
        })))
        .await;
}

fn strip_bearer(value: &str) -> &str {
    value
        .get(..7)
        .filter(|prefix| prefix.eq_ignore_ascii_case("bearer "))
        .and_then(|_| value.get(7..))
        .unwrap_or(value)
}

struct ValidatedQuery {
    mailbox: Option<String>,
    limit: usize,
    cursor: Option<GatewayMessageCursor>,
    mailbox_role: Option<String>,
    unread: Option<bool>,
    since: Option<String>,
    before: Option<String>,
    q: Option<String>,
}

fn validate_query(input: MessageQueryInput) -> Result<ValidatedQuery, GatewayFailure> {
    if input
        .mailbox
        .as_ref()
        .is_some_and(|value| !valid_email(value))
        || input
            .q
            .as_ref()
            .is_some_and(|value| utf16(value.trim()) > 200)
        || input
            .cursor
            .as_ref()
            .is_some_and(|value| value.len() > 1000 || value.is_empty())
        || input.mailbox_role.as_deref().is_some_and(|value| {
            !matches!(
                value,
                "inbox" | "sent" | "archive" | "drafts" | "trash" | "junk" | "custom"
            )
        })
    {
        return Err(GatewayFailure::invalid());
    }
    let limit = input
        .limit
        .as_deref()
        .unwrap_or("25")
        .parse::<usize>()
        .ok()
        .filter(|value| (1..=100).contains(value))
        .ok_or_else(GatewayFailure::invalid)?;
    let unread = match input.unread.as_deref() {
        None => None,
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => return Err(GatewayFailure::invalid()),
    };
    let since = input.since.as_deref().map(normalize_date).transpose()?;
    let before = input.before.as_deref().map(normalize_date).transpose()?;
    if since
        .as_ref()
        .zip(before.as_ref())
        .is_some_and(|(since, before)| since >= before)
    {
        return Err(GatewayFailure::invalid());
    }
    let cursor = input.cursor.as_deref().map(decode_cursor).transpose()?;
    Ok(ValidatedQuery {
        mailbox: input.mailbox,
        limit,
        cursor,
        mailbox_role: input.mailbox_role,
        unread,
        since,
        before,
        q: input.q.map(|value| value.trim().to_string()),
    })
}

#[derive(Serialize, Deserialize)]
struct CursorValue {
    date: String,
    id: String,
}

fn decode_cursor(value: &str) -> Result<GatewayMessageCursor, GatewayFailure> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| GatewayFailure::cursor())?;
    let value =
        serde_json::from_slice::<CursorValue>(&decoded).map_err(|_| GatewayFailure::cursor())?;
    if value.date.is_empty() || value.id.is_empty() {
        return Err(GatewayFailure::cursor());
    }
    Ok(GatewayMessageCursor {
        date: value.date,
        id: value.id,
    })
}

fn encode_cursor(message: &MessageReadModel) -> Result<String, GatewayStorageError> {
    serde_json::to_vec(&CursorValue {
        date: message.date.clone(),
        id: message.id.clone(),
    })
    .map(|value| URL_SAFE_NO_PAD.encode(value))
    .map_err(|_| GatewayStorageError::Internal)
}

fn normalize_date(value: &str) -> Result<String, GatewayFailure> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| {
            value
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        })
        .map_err(|_| GatewayFailure::invalid())
}

fn message_summary(message: &MessageReadModel, account_email: &str) -> Value {
    let attachments = message
        .attachments
        .as_array()
        .map(|values| {
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let mut value = value.clone();
                    if let Some(object) = value.as_object_mut() {
                        object
                            .entry("index")
                            .or_insert_with(|| Value::from(index as u64));
                    }
                    value
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "id": message.id,
        "accountEmail": account_email,
        "folder": message.mailbox,
        "mailboxRole": message.mailbox_role,
        "from": message.from,
        "to": message.to,
        "subject": message.subject,
        "preview": message.preview,
        "date": message.date,
        "unread": message.unread,
        "flagged": message.flagged,
        "hasAttachments": message.has_attachments,
        "attachments": attachments,
        "labels": message.labels,
    })
}

fn message_has_recipient(value: &Value, email: &str) -> bool {
    let matches = |value: &Value| match value {
        Value::String(address) => address.eq_ignore_ascii_case(email),
        Value::Object(recipient) => recipient
            .get("address")
            .and_then(Value::as_str)
            .is_some_and(|address| address.eq_ignore_ascii_case(email)),
        _ => false,
    };
    value.as_array().map_or_else(
        || matches(value),
        |recipients| recipients.iter().any(matches),
    )
}

fn valid_send(input: &SendInput) -> bool {
    valid_email(&input.mailbox)
        && (!input.to.is_empty()
            || input.cc.as_ref().is_some_and(|values| !values.is_empty())
            || !input.bcc.is_empty())
        && input.envelope().is_valid()
        && input.to.len() <= 100
        && input.to.iter().all(|value| valid_email(value))
        && input.cc.as_ref().map_or(true, |values| {
            values.len() <= 100 && values.iter().all(|value| valid_email(value))
        })
        && !input.subject.trim().is_empty()
        && utf16(input.subject.trim()) <= 500
        && !input.text.is_empty()
        && utf16(&input.text) <= 2_000_000
        && input
            .html
            .as_ref()
            .map_or(true, |value| utf16(value) <= 2_000_000)
}

fn valid_email(value: &str) -> bool {
    let value = value.trim();
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !value.chars().any(char::is_whitespace)
        && value.len() <= 320
}

fn utf16(value: &str) -> usize {
    value.encode_utf16().count()
}

enum GatewayStorageError {
    Storage,
    Application,
    Unauthorized,
    Disabled,
    Mailbox,
    Message,
    Attachment,
    Internal,
    Join,
}

impl From<AuthStoreError> for GatewayStorageError {
    fn from(_: AuthStoreError) -> Self {
        Self::Storage
    }
}

impl From<imail_core::ApplicationError<AuthStoreError>> for GatewayStorageError {
    fn from(_: imail_core::ApplicationError<AuthStoreError>) -> Self {
        Self::Application
    }
}

async fn run<T: Send + 'static>(
    database: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore) -> Result<T, GatewayStorageError> + Send + 'static,
) -> Result<T, GatewayStorageError> {
    tokio::task::spawn_blocking(move || {
        let mut store = SqliteAuthStore::open_database(database)?;
        operation(&mut store)
    })
    .await
    .map_err(|_| GatewayStorageError::Join)?
}

struct GatewayFailure {
    status: StatusCode,
    code: String,
    message: String,
}

impl GatewayFailure {
    fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            code: code.into(),
            message: message.into(),
        }
    }

    fn invalid() -> Self {
        Self::new(400, "INVALID_REQUEST", "请求参数无效")
    }

    fn cursor() -> Self {
        Self::new(400, "INVALID_CURSOR", "分页游标无效或已损坏")
    }

    fn unauthorized() -> Self {
        Self::new(401, "UNAUTHORIZED", "Token 无效、已过期或缺少权限")
    }

    fn internal() -> Self {
        Self::new(500, "INTERNAL_ERROR", "网关处理请求时发生错误")
    }
}

fn gateway_error(request_id: &str, error: GatewayFailure) -> Response {
    let value = Map::from_iter([
        ("code".into(), Value::String(error.code)),
        ("message".into(), Value::String(error.message)),
        ("requestId".into(), Value::String(request_id.into())),
    ]);
    (error.status, Json(json!({ "error": value }))).into_response()
}

fn gateway_mail_error(request_id: &str, error: MailApplicationError<AuthStoreError>) -> Response {
    let status = error.status();
    if status < 500 {
        gateway_error(
            request_id,
            GatewayFailure::new(status, error.code(), error.to_string()),
        )
    } else {
        gateway_error(request_id, GatewayFailure::internal())
    }
}

async fn not_found(Extension(request_id): Extension<RequestId>) -> Response {
    gateway_error(
        &request_id.0,
        GatewayFailure::new(404, "ENDPOINT_NOT_FOUND", "开发者网关接口不存在"),
    )
}

async fn openapi() -> Response {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(openapi_document()),
    )
        .into_response()
}

async fn docs() -> Response {
    let html = "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>iMail Developer Gateway</title><style>body{font:16px/1.6 system-ui;margin:0;background:#111;color:#eee}main{max-width:900px;margin:auto;padding:64px 24px}code,pre{background:#1d1d1d;border:1px solid #333;border-radius:8px}code{padding:2px 6px}pre{padding:18px;overflow:auto}a{color:#7dd3fc}</style></head><body><main><h1>iMail Developer Gateway</h1><p>使用短期 Bearer Token 按授权邮箱读取和发送邮件。</p><p><a href=\"/gateway/openapi.json\">OpenAPI 3.1 JSON</a></p><pre>Authorization: Bearer imail_your_token<br>GET /gateway/v1/mailboxes<br>GET /gateway/v1/messages</pre><p>WebSocket 事件地址：<code>/gateway/v1/events</code></p></main></body></html>";
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self' ws: wss:; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'",
            ),
        ],
        Html(html),
    )
        .into_response()
}

fn openapi_document() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "iMail Developer Gateway",
            "version": "1.0.0",
            "description": "使用短期 Token 按邮箱地址安全读取、发送邮件，并通过 WebSocket 接收新邮件事件。"
        },
        "servers": [{ "url": "/gateway/v1", "description": "当前 iMail 实例" }],
        "components": {
            "securitySchemes": {
                "bearerAuth": { "type": "http", "scheme": "bearer", "bearerFormat": "iMail Token" }
            },
            "schemas": {
                "ErrorResponse": { "type": "object", "required": ["error"] },
                "MessageSummary": { "type": "object", "required": ["id", "accountEmail", "folder", "mailboxRole", "from", "to", "subject", "preview", "date", "unread", "flagged", "hasAttachments", "attachments", "labels"] },
                "Page": { "type": "object", "required": ["limit", "count", "hasMore", "nextCursor"] }
            }
        },
        "paths": {
            "/health": { "get": { "operationId": "gatewayHealth", "security": [], "responses": { "200": { "description": "网关正常" } } } },
            "/mailboxes": { "get": { "operationId": "listMailboxes", "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "邮箱列表" }, "401": { "description": "Token 无效或权限不足" } } } },
            "/messages": { "get": { "operationId": "listMessages", "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "邮件分页结果" }, "400": { "description": "参数或游标无效" } } } },
            "/mailboxes/{mailbox}/messages": { "get": { "operationId": "listMailboxMessages", "description": "按主邮箱或已同步的 iCloud Hide My Email 地址查询；隐私邮箱查询只返回实际发送到该地址的邮件，且不暴露主 iCloud 地址。", "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "邮件分页结果" } } } },
            "/messages/{messageId}": { "get": { "operationId": "getMessage", "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "邮件详情" } } } },
            "/messages/{messageId}/attachments/{index}": { "get": { "operationId": "downloadAttachment", "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "附件二进制内容" } } } },
            "/send": { "post": { "operationId": "sendMessage", "security": [{ "bearerAuth": [] }], "responses": { "201": { "description": "发送成功" } } } }
        },
        "x-websocket": {
            "url": "/gateway/v1/events",
            "authentication": { "firstMessage": { "type": "authenticate", "token": "imail_your_token" }, "requiredScope": "messages:read" },
            "messages": { "connected": { "example": { "type": "connected", "occurredAt": "2026-07-29T10:00:00.000Z" } }, "messageCreated": { "type": "message.created" } }
        }
    })
}

#[cfg(test)]
mod composition_tests {
    use super::*;

    #[test]
    fn bcc_only_send_preserves_reply_headers_and_rejects_injection() {
        let mut value = json!({ "mailbox": "me@example.test", "to": [],
            "bcc": ["hidden@example.test"], "subject": "Reply", "text": "Body",
            "inReplyTo": ["<parent@example.test>"], "references": ["<root@example.test>"] });
        let input: SendInput = serde_json::from_value(value.clone()).unwrap();
        assert!(valid_send(&input));
        assert_eq!(input.envelope().bcc, ["hidden@example.test"]);
        assert_eq!(
            input.envelope().reply.in_reply_to,
            ["<parent@example.test>"]
        );
        value["inReplyTo"] = json!(["<parent@example.test>\r\nBcc: attacker@example.test"]);
        assert!(!valid_send(&serde_json::from_value(value.clone()).unwrap()));
        value["unknown"] = json!(true);
        assert!(serde_json::from_value::<SendInput>(value).is_err());
    }
}
