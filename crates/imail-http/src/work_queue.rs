use std::{collections::HashSet, sync::Arc};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Extension, Json, Router,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use imail_core::{drafts::DraftService, AccountRepository, MessageRepository};
use imail_protocol::{
    normalize_message_id, ComposeEnvelope, DraftInput, MailAddressView, MailWorkStatus,
    ReplyHeaders, SendAttachmentInput, SendMessageInput,
};
use imail_storage_sqlite::{AuthStoreError, SqliteAuthStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{auth::AuthenticatedUser, AppState, EmbeddedOperationError};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/mail-work-items", get(list))
        .route("/api/messages/:id/work-item", put(set).delete(complete))
        .route("/api/messages/:id/reply-draft", post(reply_draft))
        .route("/api/drafts/:id/schedule", post(schedule_draft_handler))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetInput {
    status: MailWorkStatus,
    due_at: Option<String>,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListQuery {
    status: Option<MailWorkStatus>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum ReplyMode {
    Reply,
    ReplyAll,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplyDraftInput {
    mode: ReplyMode,
    text: String,
    html: Option<String>,
    subject: Option<String>,
    #[serde(default)]
    bcc: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScheduleDraftInput {
    request_id: String,
    send_at: String,
    confirmed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAttachment {
    filename: String,
    content_type: String,
    data: String,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

fn error(status: u16, message: impl Into<String>) -> EmbeddedOperationError {
    EmbeddedOperationError {
        status,
        message: message.into(),
    }
}

fn storage_error(cause: AuthStoreError) -> EmbeddedOperationError {
    match cause {
        AuthStoreError::ContentOwnershipViolation => error(404, "邮件不存在"),
        AuthStoreError::InvalidContentData => error(400, "处理队列参数无效"),
        other => {
            eprintln!("[imail-http] mail work queue storage error: {other}");
            error(500, "邮件处理队列暂时不可用")
        }
    }
}

fn validate_due_at(value: Option<String>) -> Result<Option<String>, EmbeddedOperationError> {
    let Some(value) = value else { return Ok(None) };
    let parsed = DateTime::parse_from_rfc3339(&value)
        .map_err(|_| error(400, "处理期限无效"))?
        .with_timezone(&Utc);
    if parsed < Utc::now() - Duration::days(365) || parsed > Utc::now() + Duration::days(3650) {
        return Err(error(400, "处理期限超出允许范围"));
    }
    Ok(Some(parsed.to_rfc3339_opts(SecondsFormat::Millis, true)))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn execute(
    store: &mut SqliteAuthStore,
    owner: &str,
    operation: &str,
    message_id: Option<&str>,
    input: Option<Value>,
) -> Result<Value, EmbeddedOperationError> {
    match operation {
        "list" => {
            let query: ListQuery = serde_json::from_value(input.unwrap_or_else(|| json!({})))
                .map_err(|_| error(400, "处理队列筛选无效"))?;
            let items = store
                .list_mail_work_items(owner, query.status)
                .map_err(storage_error)?;
            let mut views = Vec::with_capacity(items.len());
            for item in items {
                if let Some(message) = store
                    .message(owner, &item.message_id)
                    .map_err(storage_error)?
                {
                    views.push(json!({"item":item,"message":message}));
                }
            }
            Ok(json!({"items":views}))
        }
        "set" => {
            let input: SetInput =
                serde_json::from_value(input.ok_or_else(|| error(400, "缺少处理队列参数"))?)
                    .map_err(|_| error(400, "处理队列参数无效"))?;
            if input.note.encode_utf16().count() > 4_000 {
                return Err(error(400, "处理备注不能超过 4000 个字符"));
            }
            let due_at = validate_due_at(input.due_at)?;
            let item = store
                .set_mail_work_item(
                    owner,
                    &Uuid::new_v4().to_string(),
                    message_id.unwrap_or_default(),
                    input.status,
                    due_at.as_deref(),
                    input.note.trim(),
                    None,
                    &now(),
                )
                .map_err(storage_error)?;
            Ok(json!({"item":item}))
        }
        "complete" => {
            let completed = store
                .complete_mail_work_item(owner, message_id.unwrap_or_default())
                .map_err(storage_error)?;
            if !completed {
                return Err(error(404, "处理队列项目不存在"));
            }
            Ok(json!({"completed":true,"messageId":message_id.unwrap_or_default()}))
        }
        "replyDraft" => create_reply_draft(
            store,
            owner,
            message_id.unwrap_or_default(),
            input.ok_or_else(|| error(400, "缺少回复草稿参数"))?,
        ),
        "scheduleDraft" => schedule_draft(
            store,
            owner,
            message_id.unwrap_or_default(),
            input.ok_or_else(|| error(400, "缺少发送确认参数"))?,
        ),
        _ => Err(error(404, "邮件处理队列操作不存在")),
    }
}

pub(crate) fn create_reply_draft(
    store: &mut SqliteAuthStore,
    owner: &str,
    message_id: &str,
    input: Value,
) -> Result<Value, EmbeddedOperationError> {
    let input: ReplyDraftInput =
        serde_json::from_value(input).map_err(|_| error(400, "回复草稿参数无效"))?;
    if input.text.trim().is_empty()
        || input.text.encode_utf16().count() > 2_000_000
        || input
            .html
            .as_ref()
            .is_some_and(|value| value.encode_utf16().count() > 8_000_000)
        || input
            .subject
            .as_ref()
            .is_some_and(|value| value.encode_utf16().count() > 500)
    {
        return Err(error(400, "回复正文或主题无效"));
    }
    let original = store
        .message(owner, message_id)
        .map_err(storage_error)?
        .ok_or_else(|| error(404, "邮件不存在"))?;
    let accounts = store.list_accounts(owner).map_err(storage_error)?;
    let mut seen = accounts
        .iter()
        .map(|account| account.email.trim().to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let from: MailAddressView = serde_json::from_value(original.from.clone())
        .map_err(|_| error(400, "原邮件发件人无效"))?;
    let targets = if original.headers.reply_to.is_empty() {
        vec![from]
    } else {
        original.headers.reply_to.clone()
    };
    let mut take = |values: Vec<MailAddressView>| {
        values
            .into_iter()
            .filter_map(|value| {
                let address = value.address.trim().to_string();
                let key = address.to_ascii_lowercase();
                if imail_protocol::valid_mail_address(&address) && seen.insert(key) {
                    Some(address)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    let mut to = take(targets);
    let mut cc = Vec::new();
    if matches!(input.mode, ReplyMode::ReplyAll) {
        let original_to: Vec<MailAddressView> =
            serde_json::from_value(original.to.clone()).unwrap_or_default();
        to.extend(take(original_to));
        cc = take(original.headers.cc.clone());
    }
    if to.is_empty() && cc.is_empty() {
        return Err(error(400, "原邮件没有可回复的外部收件人"));
    }
    let parent = original
        .message_id
        .as_deref()
        .and_then(normalize_message_id);
    let mut references = original
        .headers
        .reply
        .references
        .iter()
        .filter_map(|value| normalize_message_id(value))
        .collect::<Vec<_>>();
    if let Some(parent) = &parent {
        if !references.contains(parent) {
            references.push(parent.clone());
        }
    }
    if references.len() > 100 {
        references.drain(..references.len() - 100);
    }
    let subject = input.subject.unwrap_or_else(|| {
        if original
            .subject
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("re:")
        {
            original.subject.clone()
        } else {
            format!("Re: {}", original.subject)
        }
    });
    let envelope = ComposeEnvelope {
        bcc: input.bcc,
        reply: ReplyHeaders {
            in_reply_to: parent.into_iter().collect(),
            references,
        },
    };
    if !envelope.is_valid() {
        return Err(error(400, "回复关联或密送地址无效"));
    }
    let created_at = now();
    let draft_id = Uuid::new_v4().to_string();
    let draft = DraftService::new(store)
        .create(
            owner,
            &draft_id,
            &created_at,
            DraftInput {
                envelope,
                account_id: original.account_id.clone(),
                to: json!(to),
                cc: json!(cc),
                subject,
                text: input.text,
                html: input.html.unwrap_or_default(),
                attachments: json!([]),
            },
        )
        .map_err(|_| error(500, "回复草稿保存失败"))?;
    let item = store
        .set_mail_work_item(
            owner,
            &Uuid::new_v4().to_string(),
            message_id,
            MailWorkStatus::NeedsReview,
            None,
            "Agent 已创建回复草稿，等待确认",
            Some(&draft_id),
            &created_at,
        )
        .map_err(storage_error)?;
    Ok(json!({"draft":draft,"item":item}))
}

pub(crate) fn schedule_draft(
    store: &mut SqliteAuthStore,
    owner: &str,
    draft_id: &str,
    input: Value,
) -> Result<Value, EmbeddedOperationError> {
    let input: ScheduleDraftInput =
        serde_json::from_value(input).map_err(|_| error(400, "发送确认参数无效"))?;
    if !input.confirmed {
        return Err(error(409, "安排发送前必须明确确认 confirmed=true"));
    }
    let draft = DraftService::new(store)
        .get(owner, draft_id)
        .map_err(|_| error(404, "草稿不存在"))?;
    let to: Vec<String> =
        serde_json::from_value(draft.to.clone()).map_err(|_| error(400, "草稿收件人无效"))?;
    let cc: Vec<String> =
        serde_json::from_value(draft.cc.clone()).map_err(|_| error(400, "草稿抄送人无效"))?;
    let stored: Vec<StoredAttachment> = serde_json::from_value(draft.attachments.clone())
        .map_err(|_| error(400, "草稿附件无效"))?;
    let message = SendMessageInput {
        envelope: draft.envelope.clone(),
        account_id: draft.account_id,
        to,
        cc: (!cc.is_empty()).then_some(cc),
        subject: draft.subject,
        text: draft.text,
        html: (!draft.html.is_empty()).then_some(draft.html),
        attachments: (!stored.is_empty()).then(|| {
            stored
                .into_iter()
                .map(|value| SendAttachmentInput {
                    filename: value.filename,
                    content_type: value.content_type,
                    data: value.data,
                })
                .collect()
        }),
    };
    let result = crate::outbox::schedule_message(
        store,
        owner,
        &input.request_id,
        &input.send_at,
        message,
        Some(draft_id),
    )?;
    if let Some(item) = store
        .list_mail_work_items(owner, None)
        .map_err(storage_error)?
        .into_iter()
        .find(|item| item.draft_id.as_deref() == Some(draft_id))
    {
        let _ = store.set_mail_work_item(
            owner,
            &item.id,
            &item.message_id,
            MailWorkStatus::Waiting,
            None,
            "回复已确认并加入发件箱",
            Some(draft_id),
            &now(),
        );
    }
    Ok(result)
}

async fn run(
    state: Arc<AppState>,
    owner: String,
    operation: &'static str,
    id: Option<String>,
    input: Option<Value>,
    success_status: StatusCode,
) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        let mut store = SqliteAuthStore::open_database(state.config.data_dir.join("imail.sqlite"))
            .map_err(storage_error)?;
        execute(&mut store, &owner, operation, id.as_deref(), input)
    })
    .await;
    match result {
        Ok(Ok(value)) => (success_status, Json(value)).into_response(),
        Ok(Err(cause)) => (
            StatusCode::from_u16(cause.status).unwrap_or(StatusCode::BAD_REQUEST),
            Json(ErrorBody {
                error: cause.message,
            }),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "邮件处理任务中断".into(),
            }),
        )
            .into_response(),
    }
}

async fn list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<ListQuery>,
) -> Response {
    run(
        state,
        user.user_id,
        "list",
        None,
        Some(json!(query)),
        StatusCode::OK,
    )
    .await
}

async fn set(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(input): Json<SetInput>,
) -> Response {
    run(
        state,
        user.user_id,
        "set",
        Some(id),
        Some(json!(input)),
        StatusCode::OK,
    )
    .await
}

async fn complete(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    run(
        state,
        user.user_id,
        "complete",
        Some(id),
        None,
        StatusCode::OK,
    )
    .await
}

async fn reply_draft(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(input): Json<ReplyDraftInput>,
) -> Response {
    run(
        state,
        user.user_id,
        "replyDraft",
        Some(id),
        Some(json!(input)),
        StatusCode::CREATED,
    )
    .await
}

async fn schedule_draft_handler(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(input): Json<ScheduleDraftInput>,
) -> Response {
    run(
        state,
        user.user_id,
        "scheduleDraft",
        Some(id),
        Some(json!(input)),
        StatusCode::CREATED,
    )
    .await
}
