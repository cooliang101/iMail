use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Extension, Json, Router,
};
use chrono::{SecondsFormat, Utc};
use imail_core::{drafts::DraftService, ApplicationError};
use imail_protocol::{DraftInput, DraftReadModel};
use imail_storage_sqlite::{AuthStoreError, SqliteAuthStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{auth::AuthenticatedUser, AppState};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/drafts", get(list).post(create))
        .route("/api/drafts/:id", put(update).delete(remove))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DraftAttachment {
    id: String,
    filename: String,
    content_type: String,
    size: u64,
    data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DraftPayload {
    #[serde(default, flatten)]
    envelope: imail_protocol::ComposeEnvelope,
    account_id: String,
    #[serde(default)]
    to: Vec<String>,
    #[serde(default)]
    cc: Vec<String>,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    html: String,
    #[serde(default)]
    attachments: Vec<DraftAttachment>,
}

impl DraftPayload {
    fn validated(mut self) -> Option<DraftInput> {
        if !self.envelope.is_valid()
            || Uuid::parse_str(&self.account_id).is_err()
            || !valid_addresses(&mut self.to)
            || !valid_addresses(&mut self.cc)
            || utf16_len(&self.subject) > 500
            || utf16_len(&self.text) > 2_000_000
            || utf16_len(&self.html) > 8_000_000
            || self.attachments.len() > 10
        {
            return None;
        }
        let mut total_size = 0_u64;
        for attachment in &self.attachments {
            if attachment.id.is_empty()
                || utf16_len(&attachment.id) > 100
                || attachment.filename.is_empty()
                || utf16_len(&attachment.filename) > 255
                || attachment.content_type.is_empty()
                || utf16_len(&attachment.content_type) > 150
                || attachment.size > 5 * 1024 * 1024
                || utf16_len(&attachment.data) > 7_000_000
            {
                return None;
            }
            total_size = total_size.checked_add(attachment.size)?;
        }
        if total_size > 15 * 1024 * 1024 {
            return None;
        }
        Some(DraftInput {
            envelope: self.envelope,
            account_id: self.account_id,
            to: json!(self.to),
            cc: json!(self.cc),
            subject: self.subject,
            text: self.text,
            html: self.html,
            attachments: serde_json::to_value(self.attachments).ok()?,
        })
    }
}

pub fn embedded_draft_input(value: Value) -> Option<DraftInput> {
    serde_json::from_value::<DraftPayload>(value)
        .ok()?
        .validated()
}

#[derive(Serialize)]
struct DraftsBody {
    drafts: Vec<DraftReadModel>,
}

#[derive(Serialize)]
struct DraftBody {
    draft: DraftReadModel,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

async fn list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        DraftService::new(store).list(&user.user_id)
    })
    .await
    {
        Ok(drafts) => Json(DraftsBody { drafts }).into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn create(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    payload: Result<Json<DraftPayload>, JsonRejection>,
) -> Response {
    let Some(input) = parse_payload(payload) else {
        return error(StatusCode::BAD_REQUEST, "请求参数无效");
    };
    let draft_id = match headers.get("x-draft-id") {
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|value| Uuid::parse_str(value).ok())
        {
            Some(id) => id.to_string(),
            None => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
        },
        None => Uuid::new_v4().to_string(),
    };
    let now = timestamp();
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        DraftService::new(store).create(&user.user_id, &draft_id, &now, input)
    })
    .await
    {
        Ok(draft) => (StatusCode::CREATED, Json(DraftBody { draft })).into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn update(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(draft_id): Path<String>,
    payload: Result<Json<DraftPayload>, JsonRejection>,
) -> Response {
    let Some(input) = parse_payload(payload) else {
        return error(StatusCode::BAD_REQUEST, "请求参数无效");
    };
    let now = timestamp();
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        DraftService::new(store).save_existing(&user.user_id, &draft_id, &now, input)
    })
    .await
    {
        Ok(draft) => Json(DraftBody { draft }).into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn remove(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(draft_id): Path<String>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        DraftService::new(store).delete(&user.user_id, &draft_id)
    })
    .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(cause) => application_error(cause),
    }
}

fn parse_payload(payload: Result<Json<DraftPayload>, JsonRejection>) -> Option<DraftInput> {
    payload.ok()?.0.validated()
}

fn valid_addresses(values: &mut [String]) -> bool {
    values.iter_mut().all(|value| {
        *value = value.trim().to_string();
        !value.is_empty() && utf16_len(value) <= 320
    })
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

async fn run<T>(
    database: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore) -> Result<T, ApplicationError<AuthStoreError>>
        + Send
        + 'static,
) -> Result<T, ApplicationError<AuthStoreError>>
where
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(move || {
        let mut store =
            SqliteAuthStore::open_database(database).map_err(ApplicationError::Repository)?;
        operation(&mut store)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(ApplicationError::Domain {
            code: "AUTH_WORKER_STOPPED",
            status: 500,
            message: "服务暂时无法完成请求",
        }),
    }
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
        ApplicationError::Repository(storage_error) => {
            eprintln!("[imail-http] drafts storage error: {storage_error}");
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
