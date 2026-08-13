use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{SecondsFormat, Utc};
use imail_core::contacts::{contact_logo_keys, PublicSuffixDomainResolver};
use imail_core::{
    accounts::AccountSecretCodec,
    mail_operations::{MailApplicationError, MailApplicationService},
    messages::{GatewayMessageCursor, MessageQuery, MessageQueryService},
    notifications::MailOverviewService,
    oauth_refresh::RefreshingConnectionService,
    ApplicationError, ContentRepository, LogoFetchAttemptRecord,
};
use imail_mail::{
    ImapPort, MailConnectionConfig, OutgoingMessage, ProtocolFailure, ProtocolStage, RemoteMailbox,
    RemoteMessageLocator, RemoteMoveConfirmation, SmtpPort,
};
use imail_oauth::{
    OAuthConfigResolver, OAuthEnvironment, OAuthProviderPortFactory, RefreshCoordinator,
};
use imail_protocol::{
    ContactReadModel, MessageMoveDestination, MessageReadModel, RemoteMessageFlagPatch,
    SendAttachmentInput, SendMessageInput,
};
use imail_security::MasterKey;
use imail_storage_sqlite::{AuthStoreError, MasterKeyCredentialCodec, SqliteAuthStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    auth::AuthenticatedUser,
    logo::{CachedLogoMeta, DiscoveredLogo, LogoSource, MissingLogoMeta, NEGATIVE_CACHE_VERSION},
    AppState,
};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/messages", get(list))
        .route("/api/message-stats", get(stats))
        .route("/api/contacts", get(contacts))
        .route("/api/contacts/logo", get(contact_logo))
        .route("/api/messages/:id", get(detail).patch(update_message))
        .route("/api/messages/:id/sender-logo", get(sender_logo))
        .route("/api/messages/:id/move", post(move_message))
        .route(
            "/api/messages/:id/attachments/:index",
            get(download_attachment),
        )
        .route("/api/labels", get(labels))
        .route("/api/notifications", get(notifications))
        .route("/api/send", post(send_message))
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageQueryInput {
    account_id: Option<String>,
    group: Option<String>,
    q: Option<String>,
    unread: Option<String>,
    flagged: Option<String>,
    has_attachments: Option<String>,
    mailbox_role: Option<String>,
    mailbox: Option<String>,
    mailbox_name: Option<String>,
    snoozed: Option<String>,
    label: Option<String>,
    limit: Option<String>,
    offset: Option<String>,
    cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessagesBody {
    messages: Vec<Value>,
    total: usize,
    next_offset: usize,
    next_cursor: Option<String>,
    has_more: bool,
}

#[derive(Serialize)]
struct MessageBody {
    message: Value,
}

pub fn embedded_message_query(
    fields: &BTreeMap<String, String>,
) -> Result<MessageQuery, &'static str> {
    validate_query(MessageQueryInput {
        account_id: fields.get("accountId").cloned(),
        group: fields.get("group").cloned(),
        q: fields.get("q").cloned(),
        unread: fields.get("unread").cloned(),
        flagged: fields.get("flagged").cloned(),
        has_attachments: fields.get("hasAttachments").cloned(),
        mailbox_role: fields.get("mailboxRole").cloned(),
        mailbox: fields.get("mailbox").cloned(),
        mailbox_name: fields.get("mailboxName").cloned(),
        snoozed: fields.get("snoozed").cloned(),
        label: fields.get("label").cloned(),
        limit: fields.get("limit").cloned(),
        offset: fields.get("offset").cloned(),
        cursor: fields.get("cursor").cloned(),
    })
    .map_err(|_| "请求参数无效")
}

pub fn embedded_message_page(
    page: imail_core::messages::MessagePage,
    query: &MessageQuery,
    contacts: &[ContactReadModel],
) -> Value {
    let logos = contact_map(contacts);
    let has_more = page.has_more;
    let messages = page
        .messages
        .into_iter()
        .map(|message| message_view(message, &logos, true))
        .collect::<Vec<_>>();
    json!({
        "nextOffset": query.offset.saturating_add(messages.len()),
        "nextCursor": if has_more { messages.last().and_then(message_cursor) } else { None },
        "hasMore": has_more,
        "messages": messages,
        "total": page.total,
    })
}

pub fn embedded_message_detail(message: MessageReadModel, contacts: &[ContactReadModel]) -> Value {
    json!({"message": message_view(message, &contact_map(contacts), false)})
}

pub fn embedded_contacts(contacts: Vec<ContactReadModel>) -> Value {
    json!({"contacts": contacts.into_iter().map(contact_view).collect::<Vec<_>>()})
}

#[derive(Serialize)]
struct ContactsBody {
    contacts: Vec<Value>,
}

#[derive(Serialize)]
struct LabelsBody {
    labels: Vec<String>,
}

#[derive(Serialize)]
struct NotificationsBody {
    notifications: Vec<imail_protocol::NotificationView>,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Deserialize)]
struct ContactLogoQuery {
    address: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessagePatchInput {
    unread: Option<bool>,
    flagged: Option<bool>,
    labels: Option<Vec<String>>,
    snoozed_until: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveInput {
    destination: MessageMoveDestination,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendInput {
    account_id: String,
    to: Vec<String>,
    cc: Option<Vec<String>>,
    subject: String,
    text: String,
    html: Option<String>,
    attachments: Option<Vec<HttpSendAttachment>>,
    draft_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpSendAttachment {
    id: String,
    filename: String,
    content_type: String,
    size: usize,
    data: String,
}

struct ValidatedSend {
    message: SendMessageInput,
    draft_id: Option<String>,
}

struct ValidatedPatch {
    unread: Option<bool>,
    flagged: Option<bool>,
    labels: Option<Vec<String>>,
    snoozed_until: Option<Option<String>>,
}

async fn list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(input): Query<MessageQueryInput>,
) -> Response {
    let query = match validate_query(input) {
        Ok(query) => query,
        Err(()) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    let database = state.config.data_dir.join("imail.sqlite");
    let owner = user.user_id;
    match run(database, move |store| {
        let page = MessageQueryService::new(&*store).query(&owner, &query, &now())?;
        let contacts = store
            .list_contacts(&owner)
            .map_err(ApplicationError::Repository)?;
        let logos = contact_map(&contacts);
        let has_more = page.has_more;
        let messages = page
            .messages
            .into_iter()
            .map(|message| message_view(message, &logos, true))
            .collect::<Vec<_>>();
        Ok(MessagesBody {
            next_offset: query.offset.saturating_add(messages.len()),
            next_cursor: if has_more {
                messages.last().and_then(message_cursor)
            } else {
                None
            },
            has_more,
            messages,
            total: page.total,
        })
    })
    .await
    {
        Ok(body) => Json(body).into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn detail(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(message_id): Path<String>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    let owner = user.user_id;
    match run(database, move |store| {
        let message = MessageQueryService::new(&*store).get(&owner, &message_id)?;
        let contacts = store
            .list_contacts(&owner)
            .map_err(ApplicationError::Repository)?;
        Ok(message_view(message, &contact_map(&contacts), false))
    })
    .await
    {
        Ok(message) => Json(MessageBody { message }).into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn update_message(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(message_id): Path<String>,
    Json(input): Json<MessagePatchInput>,
) -> Response {
    let patch = match validate_patch(input) {
        Ok(patch) => patch,
        Err(()) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    let data_dir = state.config.data_dir.clone();
    let owner = user.user_id;
    let environment = state.config.oauth_environment.clone();
    let coordinator = Arc::clone(&state.refresh_coordinator);
    let oauth_factory = Arc::clone(&state.config.oauth_provider_factory);
    let oauth_resolver = Arc::clone(&state.config.oauth_config_resolver);
    let transport = Arc::clone(&state.config.mail_transport_factory);
    match run_mail(data_dir, move |store, key| {
        let mut message = MessageQueryService::new(&*store).get(&owner, &message_id)?;
        let codec = MasterKeyCredentialCodec::new(key);
        if patch.unread.is_some() || patch.flagged.is_some() {
            refresh_account(
                store,
                &codec,
                &owner,
                &message.account_id,
                coordinator.as_ref(),
                &environment,
                (oauth_factory.as_ref(), oauth_resolver.as_ref()),
            )?;
            let mut imap = transport.create_imap().map_err(mail_unavailable)?;
            let mut unused_smtp = UnusedSmtp;
            MailApplicationService::new(&*store, &codec, imap.as_mut(), &mut unused_smtp)
                .update_flags(
                    &owner,
                    &message_id,
                    &RemoteMessageFlagPatch {
                        unread: patch.unread,
                        flagged: patch.flagged,
                    },
                )?;
        }
        if let Some(value) = patch.unread {
            message.unread = value;
        }
        if let Some(value) = patch.flagged {
            message.flagged = value;
        }
        if let Some(labels) = patch.labels {
            message.labels = json!(labels);
        }
        if let Some(value) = patch.snoozed_until {
            message.snoozed_until = value;
        }
        store
            .upsert_message(&owner, &message)
            .map_err(MailApplicationError::Repository)?;
        let contacts = store
            .list_contacts(&owner)
            .map_err(MailApplicationError::Repository)?;
        Ok(message_view(message, &contact_map(&contacts), false))
    })
    .await
    {
        Ok(message) => Json(MessageBody { message }).into_response(),
        Err(cause) => mail_error(cause),
    }
}

async fn move_message(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(message_id): Path<String>,
    Json(input): Json<MoveInput>,
) -> Response {
    let data_dir = state.config.data_dir.clone();
    let owner = user.user_id;
    let environment = state.config.oauth_environment.clone();
    let coordinator = Arc::clone(&state.refresh_coordinator);
    let oauth_factory = Arc::clone(&state.config.oauth_provider_factory);
    let oauth_resolver = Arc::clone(&state.config.oauth_config_resolver);
    let transport = Arc::clone(&state.config.mail_transport_factory);
    match run_mail(data_dir, move |store, key| {
        let mut message = MessageQueryService::new(&*store).get(&owner, &message_id)?;
        let codec = MasterKeyCredentialCodec::new(key);
        refresh_account(
            store,
            &codec,
            &owner,
            &message.account_id,
            coordinator.as_ref(),
            &environment,
            (oauth_factory.as_ref(), oauth_resolver.as_ref()),
        )?;
        let mut imap = transport.create_imap().map_err(mail_unavailable)?;
        let mut unused_smtp = UnusedSmtp;
        let moved = MailApplicationService::new(&*store, &codec, imap.as_mut(), &mut unused_smtp)
            .move_message(&owner, &message_id, input.destination)?;
        message.mailbox = moved.mailbox.clone();
        message.mailbox_role = match input.destination {
            MessageMoveDestination::Archive => "archive",
            MessageMoveDestination::Trash => "trash",
        }
        .into();
        if let Some(uid) = moved.uid {
            message.uid = i64::from(uid);
        }
        message.snoozed_until = None;
        store
            .upsert_message(&owner, &message)
            .map_err(MailApplicationError::Repository)?;
        let contacts = store
            .list_contacts(&owner)
            .map_err(MailApplicationError::Repository)?;
        Ok(json!({
            "message": message_view(message, &contact_map(&contacts), false),
            "destination": input.destination,
            "mailbox": moved.mailbox,
        }))
    })
    .await
    {
        Ok(body) => Json(body).into_response(),
        Err(cause) => mail_error(cause),
    }
}

async fn download_attachment(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((message_id, index)): Path<(String, String)>,
) -> Response {
    let index = match index.parse::<usize>() {
        Ok(index) => index,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    match download_attachment_for(state, user.user_id, message_id, index).await {
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
        Err(cause) => mail_error(cause),
    }
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(input): Json<SendInput>,
) -> Response {
    let input = match validate_send(input) {
        Ok(input) => input,
        Err(()) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    match send_for(state, user.user_id, input.message, input.draft_id).await {
        Ok(sent) => (StatusCode::CREATED, Json(sent)).into_response(),
        Err(cause) => mail_error(cause),
    }
}

pub(crate) async fn download_attachment_for(
    state: Arc<AppState>,
    owner: String,
    message_id: String,
    index: usize,
) -> Result<imail_mail::DownloadedAttachment, MailApplicationError<AuthStoreError>> {
    let data_dir = state.config.data_dir.clone();
    let cache_dir = data_dir.clone();
    let environment = state.config.oauth_environment.clone();
    let coordinator = Arc::clone(&state.refresh_coordinator);
    let oauth_factory = Arc::clone(&state.config.oauth_provider_factory);
    let oauth_resolver = Arc::clone(&state.config.oauth_config_resolver);
    let transport = Arc::clone(&state.config.mail_transport_factory);
    run_mail(data_dir, move |store, key| {
        let message = MessageQueryService::new(&*store).get(&owner, &message_id)?;
        let cache_key = crate::attachment_cache::AttachmentCacheKey::new(&owner, &message, index);
        if let Some(attachment) = crate::attachment_cache::read(&cache_dir, &cache_key) {
            return Ok(attachment);
        }
        let codec = MasterKeyCredentialCodec::new(key);
        refresh_account(
            store,
            &codec,
            &owner,
            &message.account_id,
            coordinator.as_ref(),
            &environment,
            (oauth_factory.as_ref(), oauth_resolver.as_ref()),
        )?;
        let mut imap = transport.create_imap().map_err(mail_unavailable)?;
        let mut unused_smtp = UnusedSmtp;
        let attachment =
            MailApplicationService::new(&*store, &codec, imap.as_mut(), &mut unused_smtp)
                .download_attachment(&owner, &message_id, index)?;
        let _ = crate::attachment_cache::write(&cache_dir, &cache_key, &attachment);
        Ok(attachment)
    })
    .await
}

pub(crate) async fn embedded_download_attachment(
    state: Arc<AppState>,
    owner: String,
    message_id: String,
    index: usize,
) -> Result<Vec<u8>, crate::EmbeddedOperationError> {
    download_attachment_for(state, owner, message_id, index)
        .await
        .map(|attachment| attachment.content)
        .map_err(embedded_mail_error)
}

pub(crate) async fn send_for(
    state: Arc<AppState>,
    owner: String,
    message: SendMessageInput,
    draft_id: Option<String>,
) -> Result<imail_protocol::SendMessageResult, MailApplicationError<AuthStoreError>> {
    let data_dir = state.config.data_dir.clone();
    let environment = state.config.oauth_environment.clone();
    let coordinator = Arc::clone(&state.refresh_coordinator);
    let oauth_factory = Arc::clone(&state.config.oauth_provider_factory);
    let oauth_resolver = Arc::clone(&state.config.oauth_config_resolver);
    let transport = Arc::clone(&state.config.mail_transport_factory);
    run_mail(data_dir, move |store, key| {
        let codec = MasterKeyCredentialCodec::new(key);
        refresh_account(
            store,
            &codec,
            &owner,
            &message.account_id,
            coordinator.as_ref(),
            &environment,
            (oauth_factory.as_ref(), oauth_resolver.as_ref()),
        )?;
        let mut unused_imap = UnusedImap;
        let mut smtp = transport.create_smtp().map_err(mail_unavailable)?;
        let sent = MailApplicationService::new(&*store, &codec, &mut unused_imap, smtp.as_mut())
            .send_message(&owner, &message)?;
        if let Some(draft_id) = draft_id {
            let _ = store
                .delete_draft(&owner, &draft_id)
                .map_err(MailApplicationError::Repository)?;
        }
        Ok(sent)
    })
    .await
}

pub(crate) async fn embedded_send(
    state: Arc<AppState>,
    owner: String,
    input: Value,
) -> Result<Value, crate::EmbeddedOperationError> {
    let input = serde_json::from_value::<SendInput>(input)
        .map_err(|_| embedded_error(422, "请求参数无效"))?;
    let input = validate_send(input).map_err(|_| embedded_error(400, "请求参数无效"))?;
    let sent = send_for(state, owner, input.message, input.draft_id)
        .await
        .map_err(embedded_mail_error)?;
    serde_json::to_value(sent).map_err(|_| embedded_error(500, "服务暂时无法完成请求"))
}

pub(crate) async fn embedded_update(
    state: Arc<AppState>,
    owner: String,
    message_id: String,
    input: Value,
) -> Result<Value, crate::EmbeddedOperationError> {
    let input = serde_json::from_value::<MessagePatchInput>(input)
        .map_err(|_| embedded_error(422, "请求参数无效"))?;
    let patch = validate_patch(input).map_err(|_| embedded_error(400, "请求参数无效"))?;
    let message = update_for(
        Arc::clone(&state),
        owner.clone(),
        message_id,
        patch.unread,
        patch.flagged,
        patch.labels,
        patch.snoozed_until,
    )
    .await
    .map_err(embedded_mail_error)?;
    let contacts = run(state.config.data_dir.join("imail.sqlite"), move |store| {
        store
            .list_contacts(&owner)
            .map_err(ApplicationError::Repository)
    })
    .await
    .map_err(embedded_application_error)?;
    Ok(json!({"message":embedded_message_detail(message, &contacts)}))
}

pub(crate) async fn embedded_move(
    state: Arc<AppState>,
    owner: String,
    message_id: String,
    input: Value,
) -> Result<Value, crate::EmbeddedOperationError> {
    let input = serde_json::from_value::<MoveInput>(input)
        .map_err(|_| embedded_error(422, "请求参数无效"))?;
    let destination = input.destination;
    let (message, moved) = move_for(Arc::clone(&state), owner.clone(), message_id, destination)
        .await
        .map_err(embedded_mail_error)?;
    let contacts = run(state.config.data_dir.join("imail.sqlite"), move |store| {
        store
            .list_contacts(&owner)
            .map_err(ApplicationError::Repository)
    })
    .await
    .map_err(embedded_application_error)?;
    Ok(json!({
        "message":embedded_message_detail(message, &contacts),
        "destination":destination,
        "mailbox":moved.mailbox,
    }))
}

fn validate_patch(input: MessagePatchInput) -> Result<ValidatedPatch, ()> {
    let labels = match input.labels {
        Some(labels)
            if labels.len() <= 12
                && labels
                    .iter()
                    .all(|label| !label.trim().is_empty() && utf16(label.trim()) <= 40) =>
        {
            Some(
                labels
                    .into_iter()
                    .map(|label| label.trim().to_string())
                    .collect(),
            )
        }
        Some(_) => return Err(()),
        None => None,
    };
    let snoozed_until = match input.snoozed_until {
        Some(Value::Null) => Some(None),
        Some(Value::String(value))
            if value.ends_with('Z') && chrono::DateTime::parse_from_rfc3339(&value).is_ok() =>
        {
            Some(Some(value))
        }
        Some(_) => return Err(()),
        None => None,
    };
    Ok(ValidatedPatch {
        unread: input.unread,
        flagged: input.flagged,
        labels,
        snoozed_until,
    })
}

fn embedded_mail_error(
    cause: MailApplicationError<AuthStoreError>,
) -> crate::EmbeddedOperationError {
    if cause.status() < 500 {
        embedded_error(cause.status(), cause.to_string())
    } else {
        embedded_error(cause.status(), "邮箱服务暂时不可用")
    }
}

fn embedded_application_error(
    cause: ApplicationError<AuthStoreError>,
) -> crate::EmbeddedOperationError {
    match cause {
        ApplicationError::Domain {
            status, message, ..
        } if status < 500 => embedded_error(status, message),
        _ => embedded_error(500, "服务暂时无法完成请求"),
    }
}

fn embedded_error(status: u16, message: impl Into<String>) -> crate::EmbeddedOperationError {
    crate::EmbeddedOperationError {
        status,
        message: message.into(),
    }
}

pub(crate) async fn update_for(
    state: Arc<AppState>,
    owner: String,
    message_id: String,
    unread: Option<bool>,
    flagged: Option<bool>,
    labels: Option<Vec<String>>,
    snoozed_until: Option<Option<String>>,
) -> Result<MessageReadModel, MailApplicationError<AuthStoreError>> {
    let data_dir = state.config.data_dir.clone();
    let environment = state.config.oauth_environment.clone();
    let coordinator = Arc::clone(&state.refresh_coordinator);
    let oauth_factory = Arc::clone(&state.config.oauth_provider_factory);
    let oauth_resolver = Arc::clone(&state.config.oauth_config_resolver);
    let transport = Arc::clone(&state.config.mail_transport_factory);
    run_mail(data_dir, move |store, key| {
        let mut message = MessageQueryService::new(&*store).get(&owner, &message_id)?;
        let codec = MasterKeyCredentialCodec::new(key);
        if unread.is_some() || flagged.is_some() {
            refresh_account(
                store,
                &codec,
                &owner,
                &message.account_id,
                coordinator.as_ref(),
                &environment,
                (oauth_factory.as_ref(), oauth_resolver.as_ref()),
            )?;
            let mut imap = transport.create_imap().map_err(mail_unavailable)?;
            let mut unused_smtp = UnusedSmtp;
            MailApplicationService::new(&*store, &codec, imap.as_mut(), &mut unused_smtp)
                .update_flags(
                    &owner,
                    &message_id,
                    &RemoteMessageFlagPatch { unread, flagged },
                )?;
        }
        if let Some(value) = unread {
            message.unread = value;
        }
        if let Some(value) = flagged {
            message.flagged = value;
        }
        if let Some(value) = labels {
            message.labels = json!(value);
        }
        if let Some(value) = snoozed_until {
            message.snoozed_until = value;
        }
        store
            .upsert_message(&owner, &message)
            .map_err(MailApplicationError::Repository)?;
        Ok(message)
    })
    .await
}

pub(crate) async fn move_for(
    state: Arc<AppState>,
    owner: String,
    message_id: String,
    destination: MessageMoveDestination,
) -> Result<
    (MessageReadModel, imail_protocol::RemoteMessageMoveResult),
    MailApplicationError<AuthStoreError>,
> {
    let data_dir = state.config.data_dir.clone();
    let environment = state.config.oauth_environment.clone();
    let coordinator = Arc::clone(&state.refresh_coordinator);
    let oauth_factory = Arc::clone(&state.config.oauth_provider_factory);
    let oauth_resolver = Arc::clone(&state.config.oauth_config_resolver);
    let transport = Arc::clone(&state.config.mail_transport_factory);
    run_mail(data_dir, move |store, key| {
        let mut message = MessageQueryService::new(&*store).get(&owner, &message_id)?;
        let codec = MasterKeyCredentialCodec::new(key);
        refresh_account(
            store,
            &codec,
            &owner,
            &message.account_id,
            coordinator.as_ref(),
            &environment,
            (oauth_factory.as_ref(), oauth_resolver.as_ref()),
        )?;
        let mut imap = transport.create_imap().map_err(mail_unavailable)?;
        let mut unused_smtp = UnusedSmtp;
        let moved = MailApplicationService::new(&*store, &codec, imap.as_mut(), &mut unused_smtp)
            .move_message(&owner, &message_id, destination)?;
        message.mailbox = moved.mailbox.clone();
        message.mailbox_role = match destination {
            MessageMoveDestination::Archive => "archive",
            MessageMoveDestination::Trash => "trash",
        }
        .into();
        if let Some(uid) = moved.uid {
            message.uid = i64::from(uid);
        }
        message.snoozed_until = None;
        store
            .upsert_message(&owner, &message)
            .map_err(MailApplicationError::Repository)?;
        Ok((message, moved))
    })
    .await
}

async fn stats(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        MessageQueryService::new(&*store).stats(&user.user_id, &now())
    })
    .await
    {
        Ok(body) => Json(body).into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn contacts(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        store
            .list_contacts(&user.user_id)
            .map(|contacts| ContactsBody {
                contacts: contacts.into_iter().map(contact_view).collect(),
            })
            .map_err(ApplicationError::Repository)
    })
    .await
    {
        Ok(body) => Json(body).into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn contact_logo(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(input): Query<ContactLogoQuery>,
) -> Response {
    let address = input.address.trim().to_lowercase();
    if address.len() < 3 || utf16(&address) > 320 {
        return empty(StatusCode::BAD_REQUEST, "private, max-age=3600");
    }
    cached_logo_for_contact(state, user.user_id, address).await
}

async fn sender_logo(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(message_id): Path<String>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    let owner = user.user_id.clone();
    let address = match run(database, move |store| {
        let message = MessageQueryService::new(&*store).get(&owner, &message_id)?;
        Ok(message
            .from
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_lowercase())
    })
    .await
    {
        Ok(address) if !address.is_empty() => address,
        Ok(_) | Err(ApplicationError::Domain { status: 404, .. }) => {
            return empty(StatusCode::NOT_FOUND, "private, max-age=3600")
        }
        Err(cause) => return application_error(cause),
    };
    cached_logo_for_contact(state, user.user_id, address).await
}

async fn cached_logo_for_contact(state: Arc<AppState>, owner: String, address: String) -> Response {
    match embedded_contact_logo(state, owner, address).await {
        Ok(Some((content_type, content))) => {
            let content_type = HeaderValue::from_str(&content_type)
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type),
                    (
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("private, max-age=86400"),
                    ),
                    (
                        header::HeaderName::from_static("x-content-type-options"),
                        HeaderValue::from_static("nosniff"),
                    ),
                ],
                content,
            )
                .into_response()
        }
        _ => empty(StatusCode::NOT_FOUND, "private, max-age=3600"),
    }
}

pub(crate) async fn embedded_contact_logo(
    state: Arc<AppState>,
    owner: String,
    address: String,
) -> Result<Option<(String, Vec<u8>)>, crate::EmbeddedOperationError> {
    let data_dir = state.config.data_dir.clone();
    let database = data_dir.join("imail.sqlite");
    let lookup_owner = owner.clone();
    let lookup = run(database.clone(), move |store| {
        let contacts = store
            .list_contacts(&lookup_owner)
            .map_err(ApplicationError::Repository)?;
        let Some(contact) = contacts
            .into_iter()
            .find(|contact| contact.address.eq_ignore_ascii_case(&address))
        else {
            return Err(ApplicationError::Domain {
                code: "CONTACT_NOT_FOUND",
                status: 404,
                message: "联系人不存在",
            });
        };
        let generated = contact_logo_keys(&contact.address, &PublicSuffixDomainResolver);
        let mut keys = Vec::new();
        if let Some(key) = &contact.logo_key {
            keys.push(key.clone());
        }
        let generated = generated.ok_or(ApplicationError::Domain {
            code: "CONTACT_DOMAIN_INVALID",
            status: 404,
            message: "联系人域名无效",
        })?;
        keys.push(generated.exact.clone());
        keys.push(generated.root.clone());
        keys.dedup();
        let mut messages = store
            .list_messages(&lookup_owner)
            .map_err(ApplicationError::Repository)?
            .into_iter()
            .filter(|message| {
                message
                    .from
                    .get("address")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.trim().eq_ignore_ascii_case(&contact.address))
            })
            .collect::<Vec<_>>();
        messages.sort_by(|left, right| right.date.cmp(&left.date));
        let source = messages.first().map_or_else(
            || LogoSource {
                address: contact.address.clone(),
                name: contact.name.clone(),
                html: String::new(),
                text: String::new(),
            },
            |message| LogoSource {
                address: contact.address.clone(),
                name: message
                    .from
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                html: message.html.clone().unwrap_or_default(),
                text: message.text.clone(),
            },
        );
        Ok(LogoLookup {
            keys,
            exact_key: generated.exact,
            root_key: generated.root,
            source,
        })
    })
    .await
    .map_err(embedded_application_error)?;
    let state_for_task = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        resolve_logo(&state_for_task, &database, &data_dir, &owner, &lookup)
    })
    .await
    .map_err(|_| embedded_error(500, "服务暂时无法完成请求"))
}

struct LogoLookup {
    keys: Vec<String>,
    exact_key: String,
    root_key: String,
    source: LogoSource,
}

fn resolve_logo(
    state: &AppState,
    database: &FsPath,
    data_dir: &FsPath,
    owner: &str,
    lookup: &LogoLookup,
) -> Option<(String, Vec<u8>)> {
    match read_logo_cache(data_dir, lookup) {
        LogoCacheState::Found(value) => return Some(value),
        LogoCacheState::Missing => return None,
        LogoCacheState::Absent => {}
    }
    let lock = {
        let mut active = state.logo_in_flight.lock().ok()?;
        active
            .entry(lookup.root_key.clone())
            .or_insert_with(|| Arc::new(std::sync::Mutex::new(())))
            .clone()
    };
    let guard = lock.lock().ok()?;
    let result = resolve_logo_after_lock(state, database, data_dir, owner, lookup);
    drop(guard);
    if let Ok(mut active) = state.logo_in_flight.lock() {
        if Arc::strong_count(&lock) == 2
            && active
                .get(&lookup.root_key)
                .is_some_and(|current| Arc::ptr_eq(current, &lock))
        {
            active.remove(&lookup.root_key);
        }
    }
    result
}

fn resolve_logo_after_lock(
    state: &AppState,
    database: &FsPath,
    data_dir: &FsPath,
    owner: &str,
    lookup: &LogoLookup,
) -> Option<(String, Vec<u8>)> {
    match read_logo_cache(data_dir, lookup) {
        LogoCacheState::Found(value) => return Some(value),
        LogoCacheState::Missing => return None,
        LogoCacheState::Absent => {}
    }
    let mut store = SqliteAuthStore::open_database(database).ok()?;
    let previous = store.list_logo_fetch_attempts(owner).ok()?;
    let report = state
        .config
        .logo_discovery
        .discover(&lookup.source, &previous);
    for item in report.attempts {
        let _ = store.upsert_logo_fetch_attempt(&LogoFetchAttemptRecord {
            owner_id: owner.into(),
            target: item.target,
            domain_key: item.domain_key,
            status: item.status,
            detail: item.detail,
            attempted_at: item.attempted_at,
        });
    }
    if let Some(result) = report.result {
        if persist_logo(data_dir, &result.key, &result).is_err() {
            return None;
        }
        if result.key != lookup.root_key && read_positive_logo(data_dir, &lookup.root_key).is_none()
        {
            let _ = persist_logo(data_dir, &lookup.root_key, &result);
        }
        remember_logo(&mut store, owner, lookup, &result);
    } else {
        let _ = persist_missing(data_dir, &lookup.exact_key, report.permanent_failure);
        if lookup.root_key != lookup.exact_key
            && (report.permanent_failure || !read_missing_logo(data_dir, &lookup.root_key))
        {
            let _ = persist_missing(data_dir, &lookup.root_key, report.permanent_failure);
        }
    }
    match read_logo_cache(data_dir, lookup) {
        LogoCacheState::Found(value) => Some(value),
        LogoCacheState::Missing | LogoCacheState::Absent => None,
    }
}

fn remember_logo(
    store: &mut SqliteAuthStore,
    owner: &str,
    lookup: &LogoLookup,
    logo: &DiscoveredLogo,
) {
    let Ok(contacts) = store.list_contacts(owner) else {
        return;
    };
    let root = logo.key == lookup.root_key;
    for mut contact in contacts {
        let Some(keys) = contact_logo_keys(&contact.address, &PublicSuffixDomainResolver) else {
            continue;
        };
        if (root && keys.root != lookup.root_key) || (!root && keys.exact != lookup.exact_key) {
            continue;
        }
        contact.logo_key = Some(logo.key.clone());
        contact.logo_content_type = Some(logo.content_type.clone());
        contact.logo_source_url = Some(logo.source_url.clone());
        contact.logo_fetched_at = Some(logo.fetched_at.clone());
        let _ = store.upsert_contact(&contact);
    }
}

async fn labels(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        MailOverviewService::new(&*store)
            .labels(&user.user_id)
            .map(|labels| LabelsBody { labels })
    })
    .await
    {
        Ok(body) => Json(body).into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn notifications(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        MailOverviewService::new(&*store)
            .notifications(&user.user_id, 30, &now())
            .map(|notifications| NotificationsBody { notifications })
    })
    .await
    {
        Ok(body) => Json(body).into_response(),
        Err(cause) => application_error(cause),
    }
}

fn validate_query(input: MessageQueryInput) -> Result<MessageQuery, ()> {
    if input.q.as_ref().is_some_and(|value| utf16(value) > 200)
        || invalid_optional(&input.mailbox, 500, true)
        || invalid_optional(&input.mailbox_name, 500, true)
        || invalid_optional(&input.label, 80, false)
        || input
            .mailbox_role
            .as_deref()
            .is_some_and(|value| !valid_mailbox_role(value))
    {
        return Err(());
    }
    let unread = query_bool(input.unread)?;
    let flagged = query_bool(input.flagged)?;
    let has_attachments = query_bool(input.has_attachments)?;
    let snoozed = query_bool(input.snoozed)?;
    let limit = input
        .limit
        .as_deref()
        .unwrap_or("60")
        .parse::<usize>()
        .ok()
        .filter(|value| (1..=100).contains(value))
        .ok_or(())?;
    let offset = input
        .offset
        .as_deref()
        .unwrap_or("0")
        .parse::<usize>()
        .ok()
        .filter(|value| i64::try_from(*value).is_ok())
        .ok_or(())?;
    let cursor = input
        .cursor
        .as_deref()
        .map(decode_message_cursor)
        .transpose()?;
    let has_named_mailbox = input.mailbox.is_some() || input.mailbox_name.is_some();
    Ok(MessageQuery {
        account_id: input.account_id,
        group: input.group,
        text: input.q,
        unread,
        flagged,
        has_attachments,
        mailbox_role: if has_named_mailbox {
            None
        } else {
            Some(input.mailbox_role.unwrap_or_else(|| "inbox".into()))
        },
        mailbox: input.mailbox,
        mailbox_name: input.mailbox_name,
        snoozed,
        label: input.label,
        limit,
        offset,
        cursor,
    })
}

#[derive(Serialize, Deserialize)]
struct MessageCursorValue {
    date: String,
    id: String,
}

fn decode_message_cursor(value: &str) -> Result<GatewayMessageCursor, ()> {
    if value.is_empty() || value.len() > 1000 {
        return Err(());
    }
    let decoded = URL_SAFE_NO_PAD.decode(value).map_err(|_| ())?;
    let value: MessageCursorValue = serde_json::from_slice(&decoded).map_err(|_| ())?;
    if value.date.is_empty() || value.id.is_empty() {
        return Err(());
    }
    Ok(GatewayMessageCursor {
        date: value.date,
        id: value.id,
    })
}

fn message_cursor(message: &Value) -> Option<String> {
    let value = MessageCursorValue {
        date: message.get("date")?.as_str()?.to_string(),
        id: message.get("id")?.as_str()?.to_string(),
    };
    serde_json::to_vec(&value)
        .ok()
        .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
}

fn query_bool(value: Option<String>) -> Result<bool, ()> {
    match value.as_deref() {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        _ => Err(()),
    }
}

fn invalid_optional(value: &Option<String>, maximum: usize, required: bool) -> bool {
    value
        .as_ref()
        .is_some_and(|value| (required && value.is_empty()) || utf16(value) > maximum)
}

fn valid_mailbox_role(value: &str) -> bool {
    matches!(
        value,
        "inbox" | "sent" | "archive" | "drafts" | "trash" | "junk" | "custom"
    )
}

fn utf16(value: &str) -> usize {
    value.encode_utf16().count()
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn contact_map(contacts: &[ContactReadModel]) -> HashMap<String, Value> {
    contacts
        .iter()
        .map(|contact| (contact.address.to_lowercase(), contact_logo_view(contact)))
        .collect()
}

fn message_view(
    message: MessageReadModel,
    contacts: &HashMap<String, Value>,
    summary: bool,
) -> Value {
    let address = message
        .from
        .get("address")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut value = serde_json::to_value(message).unwrap_or_else(|_| json!({}));
    if summary {
        if let Some(object) = value.as_object_mut() {
            object.remove("text");
            object.remove("html");
        }
    }
    if let Some(from) = value.get_mut("from").and_then(Value::as_object_mut) {
        let logo = contacts
            .get(&address.to_lowercase())
            .cloned()
            .unwrap_or_else(|| json!({ "url": logo_url(&address) }));
        from.insert("logo".into(), logo);
    }
    value
}

fn contact_view(contact: ContactReadModel) -> Value {
    json!({
        "address": contact.address,
        "name": contact.name,
        "messageCount": contact.message_count,
        "lastContactAt": contact.last_contact_at,
        "logo": contact_logo_view(&contact),
    })
}

fn contact_logo_view(contact: &ContactReadModel) -> Value {
    let mut logo = Map::new();
    if let Some(value) = &contact.logo_key {
        logo.insert("key".into(), Value::String(value.clone()));
    }
    if let Some(value) = &contact.logo_content_type {
        logo.insert("contentType".into(), Value::String(value.clone()));
    }
    if let Some(value) = &contact.logo_source_url {
        logo.insert("sourceUrl".into(), Value::String(value.clone()));
    }
    if let Some(value) = &contact.logo_fetched_at {
        logo.insert("fetchedAt".into(), Value::String(value.clone()));
    }
    logo.insert("url".into(), Value::String(logo_url(&contact.address)));
    Value::Object(logo)
}

fn logo_url(address: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(address.as_bytes()).collect();
    format!("/api/contacts/logo?address={encoded}")
}

enum LogoCacheState {
    Found((String, Vec<u8>)),
    Missing,
    Absent,
}

fn read_logo_cache(data_dir: &FsPath, lookup: &LogoLookup) -> LogoCacheState {
    for key in &lookup.keys {
        if let Some(value) = read_positive_logo(data_dir, key) {
            return LogoCacheState::Found(value);
        }
    }
    if read_missing_logo(data_dir, &lookup.exact_key)
        && (lookup.root_key == lookup.exact_key || read_missing_logo(data_dir, &lookup.root_key))
    {
        LogoCacheState::Missing
    } else {
        LogoCacheState::Absent
    }
}

fn read_positive_logo(data_dir: &FsPath, key: &str) -> Option<(String, Vec<u8>)> {
    let directory = data_dir.join("sender-logos");
    let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    let meta_path = directory.join(format!("{digest}.json"));
    let image_path = directory.join(format!("{digest}.bin"));
    let (Ok(meta_info), Ok(image_info)) = (
        fs::symlink_metadata(&meta_path),
        fs::symlink_metadata(&image_path),
    ) else {
        return None;
    };
    if meta_info.file_type().is_symlink()
        || image_info.file_type().is_symlink()
        || meta_info.len() > 16 * 1024
        || image_info.len() > crate::logo::MAX_IMAGE_BYTES as u64
    {
        return None;
    }
    let meta = serde_json::from_slice::<CachedLogoMeta>(&fs::read(meta_path).ok()?).ok()?;
    if !matches!(
        meta.content_type.as_str(),
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/x-icon"
    ) {
        return None;
    }
    let content = fs::read(image_path).ok()?;
    crate::logo::image_type(&content)
        .is_some_and(|detected| detected == meta.content_type)
        .then_some((meta.content_type, content))
}

fn read_missing_logo(data_dir: &FsPath, key: &str) -> bool {
    let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    let path = data_dir.join("sender-logos").join(format!("{digest}.json"));
    let Ok(info) = fs::symlink_metadata(&path) else {
        return false;
    };
    if info.file_type().is_symlink() || info.len() > 16 * 1024 {
        return false;
    }
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let Ok(meta) = serde_json::from_slice::<MissingLogoMeta>(&bytes) else {
        return false;
    };
    meta.version == NEGATIVE_CACHE_VERSION
        && (meta.permanent || crate::logo::fresh_failure(&meta.unavailable_at, Utc::now()))
}

fn persist_logo(data_dir: &FsPath, key: &str, logo: &DiscoveredLogo) -> io::Result<()> {
    let directory = data_dir.join("sender-logos");
    fs::create_dir_all(&directory)?;
    let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    let image = directory.join(format!("{digest}.bin"));
    atomic_write(&image, &logo.content)?;
    let meta = serde_json::to_vec(&CachedLogoMeta {
        content_type: logo.content_type.clone(),
        source_url: logo.source_url.clone(),
        fetched_at: logo.fetched_at.clone(),
    })
    .map_err(io::Error::other)?;
    atomic_write(&directory.join(format!("{digest}.json")), &meta)
}

fn persist_missing(data_dir: &FsPath, key: &str, permanent: bool) -> io::Result<()> {
    let directory = data_dir.join("sender-logos");
    fs::create_dir_all(&directory)?;
    let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    let meta = serde_json::to_vec(&MissingLogoMeta {
        unavailable_at: now(),
        version: NEGATIVE_CACHE_VERSION,
        permanent,
    })
    .map_err(io::Error::other)?;
    atomic_write(&directory.join(format!("{digest}.json")), &meta)
}

fn atomic_write(path: &FsPath, content: &[u8]) -> io::Result<()> {
    let temporary = temporary_path(path);
    fs::write(&temporary, content)?;
    if let Err(error) = fs::rename(&temporary, path) {
        if path.exists() {
            fs::remove_file(path)?;
            fs::rename(&temporary, path)
        } else {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    } else {
        Ok(())
    }
}

fn temporary_path(path: &FsPath) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(format!(".{}.tmp", uuid::Uuid::new_v4()));
    PathBuf::from(value)
}

fn empty(status: StatusCode, cache: &'static str) -> Response {
    (status, [(header::CACHE_CONTROL, cache)]).into_response()
}

fn validate_send(input: SendInput) -> Result<ValidatedSend, ()> {
    if uuid::Uuid::parse_str(&input.account_id).is_err()
        || input.to.is_empty()
        || input.to.iter().any(|address| !valid_email(address))
        || input
            .cc
            .as_ref()
            .is_some_and(|addresses| addresses.iter().any(|address| !valid_email(address)))
        || input.subject.is_empty()
        || input.text.is_empty()
        || input
            .draft_id
            .as_ref()
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_err())
    {
        return Err(());
    }
    let attachments = input.attachments.unwrap_or_default();
    if attachments.len() > 10
        || attachments
            .iter()
            .map(|attachment| attachment.size)
            .sum::<usize>()
            > 15 * 1024 * 1024
        || attachments.iter().any(|attachment| {
            attachment.id.is_empty()
                || utf16(&attachment.id) > 100
                || attachment.filename.is_empty()
                || utf16(&attachment.filename) > 255
                || attachment.content_type.is_empty()
                || utf16(&attachment.content_type) > 150
                || attachment.size > 5 * 1024 * 1024
                || utf16(&attachment.data) > 7_000_000
        })
    {
        return Err(());
    }
    Ok(ValidatedSend {
        message: SendMessageInput {
            account_id: input.account_id,
            to: input.to,
            cc: input.cc,
            subject: input.subject,
            text: input.text,
            html: input.html,
            attachments: (!attachments.is_empty()).then(|| {
                attachments
                    .into_iter()
                    .map(|attachment| SendAttachmentInput {
                        filename: attachment.filename,
                        content_type: attachment.content_type,
                        data: attachment.data,
                    })
                    .collect()
            }),
        },
        draft_id: input.draft_id,
    })
}

fn valid_email(value: &str) -> bool {
    if value.is_empty() || value.len() > 320 || value.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

struct UnusedImap;

impl ImapPort for UnusedImap {
    fn verify(&mut self, _: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
        Err(unused_protocol())
    }

    fn fetch_source(
        &mut self,
        _: &MailConnectionConfig,
        _: &RemoteMessageLocator,
    ) -> Result<Vec<u8>, ProtocolFailure> {
        Err(unused_protocol())
    }

    fn update_flags(
        &mut self,
        _: &MailConnectionConfig,
        _: &RemoteMessageLocator,
        _: &RemoteMessageFlagPatch,
    ) -> Result<(), ProtocolFailure> {
        Err(unused_protocol())
    }

    fn list_mailboxes(
        &mut self,
        _: &MailConnectionConfig,
    ) -> Result<Vec<RemoteMailbox>, ProtocolFailure> {
        Err(unused_protocol())
    }

    fn move_message(
        &mut self,
        _: &MailConnectionConfig,
        _: &RemoteMessageLocator,
        _: &str,
    ) -> Result<RemoteMoveConfirmation, ProtocolFailure> {
        Err(unused_protocol())
    }
}

struct UnusedSmtp;

impl SmtpPort for UnusedSmtp {
    fn verify(&mut self, _: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
        Err(unused_protocol())
    }

    fn send(
        &mut self,
        _: &MailConnectionConfig,
        _: &OutgoingMessage,
    ) -> Result<imail_protocol::SendMessageResult, ProtocolFailure> {
        Err(unused_protocol())
    }
}

fn unused_protocol() -> ProtocolFailure {
    ProtocolFailure::from_provider(
        ProtocolStage::Imap,
        Some("UNUSED_TRANSPORT"),
        "未使用的邮件协议端口被调用",
    )
}

fn refresh_account(
    store: &mut SqliteAuthStore,
    codec: &impl AccountSecretCodec,
    owner: &str,
    account_id: &str,
    coordinator: &RefreshCoordinator,
    environment: &OAuthEnvironment,
    oauth: (&dyn OAuthProviderPortFactory, &dyn OAuthConfigResolver),
) -> Result<(), MailApplicationError<AuthStoreError>> {
    let (oauth_factory, oauth_resolver) = oauth;
    let mut oauth = oauth_factory.create();
    RefreshingConnectionService::new_with_config_resolver(
        store,
        codec,
        oauth.as_mut(),
        coordinator,
        environment,
        oauth_resolver,
    )
    .resolve(owner, account_id, Utc::now().timestamp_millis())
    .map(|_| ())
    .map_err(|_| MailApplicationError::Domain {
        code: "ACCOUNT_CONNECTION_FAILED",
        status: 502,
        message: "邮箱连接授权不可用",
    })
}

fn mail_unavailable(_: String) -> MailApplicationError<AuthStoreError> {
    MailApplicationError::Domain {
        code: "MAIL_NETWORK_UNAVAILABLE",
        status: 500,
        message: "邮箱网络运行时不可用",
    }
}

async fn run_mail<T>(
    data_dir: std::path::PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore, &MasterKey) -> Result<T, MailApplicationError<AuthStoreError>>
        + Send
        + 'static,
) -> Result<T, MailApplicationError<AuthStoreError>>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let key = MasterKey::from_file(data_dir.join("master.key")).map_err(|_| {
            MailApplicationError::Domain {
                code: "MASTER_KEY_UNAVAILABLE",
                status: 500,
                message: "服务暂时无法完成请求",
            }
        })?;
        let mut store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite"))
            .map_err(MailApplicationError::Repository)?;
        operation(&mut store, &key)
    })
    .await
    .unwrap_or(Err(MailApplicationError::Domain {
        code: "MAIL_WORKER_STOPPED",
        status: 500,
        message: "服务暂时无法完成请求",
    }))
}

pub(crate) fn mail_error(cause: MailApplicationError<AuthStoreError>) -> Response {
    let status = StatusCode::from_u16(cause.status()).unwrap_or(StatusCode::BAD_GATEWAY);
    if cause.status() < 500 {
        return error(status, cause.to_string());
    }
    error(status, "邮箱服务暂时不可用")
}

async fn run<T>(
    database: std::path::PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore) -> Result<T, ApplicationError<AuthStoreError>>
        + Send
        + 'static,
) -> Result<T, ApplicationError<AuthStoreError>>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut store =
            SqliteAuthStore::open_database(database).map_err(ApplicationError::Repository)?;
        operation(&mut store)
    })
    .await
    .unwrap_or(Err(ApplicationError::Domain {
        code: "MESSAGE_WORKER_STOPPED",
        status: 500,
        message: "服务暂时无法完成请求",
    }))
}

fn application_error(cause: ApplicationError<AuthStoreError>) -> Response {
    match cause {
        ApplicationError::Domain {
            status, message, ..
        } if status < 500 => error(
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
            message,
        ),
        ApplicationError::Domain { .. } => {
            error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求")
        }
        ApplicationError::Repository(cause) => {
            eprintln!("[imail-http] messages storage error: {cause}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求")
        }
    }
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
