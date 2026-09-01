use std::sync::Arc;
use std::{sync::mpsc, thread, time::Duration as StdDuration};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use imail_core::mail_operations::MailApplicationError;
use imail_storage_sqlite::{AuthStoreError, SqliteAuthStore};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    auth::AuthenticatedUser,
    messages::{validate_send, SendInput},
    AppState, EmbeddedOperationError,
};

pub(crate) struct OutboxRuntime {
    shutdown: mpsc::Sender<()>,
    completed: mpsc::Receiver<()>,
    thread: Option<thread::JoinHandle<()>>,
}

impl OutboxRuntime {
    pub(crate) fn start(state: Arc<AppState>) -> Result<Self, std::io::Error> {
        let (shutdown, receiver) = mpsc::channel();
        let (completed_sender, completed) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("imail-outbox".into())
            .spawn(move || {
                let mut store = None;
                loop {
                    match receiver.recv_timeout(StdDuration::from_millis(500)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if store.is_none() {
                        store = SqliteAuthStore::open_database(
                            state.config.data_dir.join("imail.sqlite"),
                        )
                        .ok();
                    }
                    let work = match store
                        .as_mut()
                        .map(|store| store.claim_due_outbox(Utc::now()))
                    {
                        Some(Ok(work)) => work,
                        Some(Err(_)) => {
                            store = None;
                            continue;
                        }
                        None => continue,
                    };
                    if let Some(work) = work {
                        let result = crate::messages::send_for_blocking(
                            &state,
                            work.owner_id,
                            work.message,
                            work.draft_id,
                        );
                        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
                        if let Some(store) = store.as_mut() {
                            match result {
                                Ok(sent) => {
                                    let _ = store.complete_outbox(&work.id, &sent.message_id, &now);
                                }
                                Err(error) => {
                                    let uncertain =
                                        matches!(error, MailApplicationError::Remote(_));
                                    let code = error.code();
                                    let detail = if uncertain {
                                        "SMTP 发送结果不确定，请核对已发送邮件后人工处理。"
                                            .to_string()
                                    } else {
                                        error.to_string().chars().take(500).collect()
                                    };
                                    let _ =
                                        store.fail_outbox(&work.id, code, &detail, uncertain, &now);
                                }
                            }
                        }
                    }
                }
                let _ = completed_sender.send(());
            })?;
        Ok(Self {
            shutdown,
            completed,
            thread: Some(thread),
        })
    }

    pub(crate) fn shutdown(&mut self, maximum_wait: StdDuration) -> bool {
        let _ = self.shutdown.send(());
        if self.completed.recv_timeout(maximum_wait).is_err() {
            return false;
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        true
    }
}

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/outbox", get(list).post(schedule))
        .route("/api/outbox/:id", axum::routing::delete(cancel))
        .route("/api/outbox/:id/retry", post(retry))
        .route("/api/outbox/:id/resolve", post(resolve))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleInput {
    send_at: String,
    #[serde(flatten)]
    message: SendInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum OutboxResolution {
    Sent,
    NotSent,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResolveInput {
    resolution: OutboxResolution,
}

fn safe_error(error: AuthStoreError) -> EmbeddedOperationError {
    match error {
        AuthStoreError::AccountNotOwned => EmbeddedOperationError {
            status: 400,
            message: "发件账户不存在或不属于当前用户".into(),
        },
        AuthStoreError::InvalidContentData => EmbeddedOperationError {
            status: 400,
            message: "待发送邮件内容无效".into(),
        },
        _ => EmbeddedOperationError {
            status: 500,
            message: "发件箱暂时不可用".into(),
        },
    }
}

fn schedule_time(value: &str, now: DateTime<Utc>) -> Result<String, EmbeddedOperationError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| EmbeddedOperationError {
            status: 400,
            message: "定时发送时间无效".into(),
        })?
        .with_timezone(&Utc);
    if parsed < now - Duration::seconds(5) || parsed > now + Duration::days(365) {
        return Err(EmbeddedOperationError {
            status: 400,
            message: "定时发送时间必须在现在至一年内".into(),
        });
    }
    Ok(parsed.to_rfc3339_opts(SecondsFormat::Millis, true))
}

pub(crate) fn execute(
    store: &mut SqliteAuthStore,
    owner: &str,
    operation: &str,
    id: Option<&str>,
    input: Option<Value>,
) -> Result<Value, EmbeddedOperationError> {
    let now = Utc::now();
    let now_text = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    match operation {
        "list" => Ok(json!({"items":store.list_outbox(owner).map_err(safe_error)?})),
        "schedule" => {
            let input: ScheduleInput =
                serde_json::from_value(input.ok_or_else(|| EmbeddedOperationError {
                    status: 400,
                    message: "缺少待发送邮件".into(),
                })?)
                .map_err(|_| EmbeddedOperationError {
                    status: 400,
                    message: "待发送邮件参数无效".into(),
                })?;
            let send_at = schedule_time(&input.send_at, now)?;
            let validated = validate_send(input.message).map_err(|_| EmbeddedOperationError {
                status: 400,
                message: "待发送邮件参数无效".into(),
            })?;
            let item = store
                .schedule_outbox(
                    owner,
                    &Uuid::new_v4().to_string(),
                    &validated.message,
                    validated.draft_id.as_deref(),
                    &send_at,
                    &now_text,
                )
                .map_err(safe_error)?;
            Ok(json!({"item":item}))
        }
        "cancel" => {
            if !store
                .cancel_outbox(owner, id.unwrap_or_default(), &now_text)
                .map_err(safe_error)?
            {
                return Err(EmbeddedOperationError {
                    status: 409,
                    message: "该邮件已进入发送阶段，无法取消".into(),
                });
            }
            Ok(json!({"cancelled":true}))
        }
        "retry" => {
            if !store
                .retry_outbox(owner, id.unwrap_or_default(), &now_text)
                .map_err(safe_error)?
            {
                return Err(EmbeddedOperationError {
                    status: 409,
                    message: "只有明确失败的邮件可以重试".into(),
                });
            }
            Ok(json!({"queued":true}))
        }
        "resolve" => {
            let input: ResolveInput =
                serde_json::from_value(input.ok_or_else(|| EmbeddedOperationError {
                    status: 400,
                    message: "缺少人工核对结果".into(),
                })?)
                .map_err(|_| EmbeddedOperationError {
                    status: 400,
                    message: "人工核对结果无效".into(),
                })?;
            let was_sent = matches!(input.resolution, OutboxResolution::Sent);
            if !store
                .resolve_outbox(owner, id.unwrap_or_default(), was_sent, &now_text)
                .map_err(safe_error)?
            {
                return Err(EmbeddedOperationError {
                    status: 409,
                    message: "只有待人工核对的邮件可以处置".into(),
                });
            }
            Ok(json!({"resolved":true,"status":if was_sent { "sent" } else { "cancelled" }}))
        }
        _ => Err(EmbeddedOperationError {
            status: 404,
            message: "发件箱操作不存在".into(),
        }),
    }
}

async fn run(
    state: Arc<AppState>,
    owner: String,
    operation: &'static str,
    id: Option<String>,
    input: Option<Value>,
) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        let mut store = SqliteAuthStore::open_database(state.config.data_dir.join("imail.sqlite"))
            .map_err(safe_error)?;
        execute(&mut store, &owner, operation, id.as_deref(), input)
    })
    .await;
    match result {
        Ok(Ok(value)) => (
            if operation == "schedule" {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            Json(value),
        )
            .into_response(),
        Ok(Err(error)) => (
            StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(json!({"error":error.message})),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"发件箱任务中断"})),
        )
            .into_response(),
    }
}

async fn list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    run(state, user.user_id, "list", None, None).await
}

async fn schedule(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(input): Json<Value>,
) -> Response {
    run(state, user.user_id, "schedule", None, Some(input)).await
}

async fn cancel(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    run(state, user.user_id, "cancel", Some(id), None).await
}

async fn retry(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    run(state, user.user_id, "retry", Some(id), None).await
}

async fn resolve(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    run(state, user.user_id, "resolve", Some(id), Some(input)).await
}
