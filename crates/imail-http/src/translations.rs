use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, post},
    Extension, Json, Router,
};
use imail_core::{translations::TranslationService, ApplicationError};
use imail_protocol::{TranslationPreparationRequest, TranslationPreparationView};
use imail_storage_sqlite::{AuthStoreError, SqliteAuthStore};
use serde::Serialize;

use crate::{auth::AuthenticatedUser, AppState};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/messages/:id/translations/prepare", post(prepare))
        .route("/api/translation-cache", delete(clear_cache))
}

#[derive(Debug)]
pub enum TranslationApplicationCall {
    Prepare {
        message_id: String,
        input: TranslationPreparationRequest,
    },
    ClearCache,
}

pub struct TranslationApplicationResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

pub async fn embedded_call(
    data_dir: PathBuf,
    user_id: String,
    call: TranslationApplicationCall,
) -> TranslationApplicationResponse {
    match call {
        TranslationApplicationCall::Prepare { message_id, input } => {
            match run(data_dir, move |store| {
                TranslationService::new(store).prepare(&user_id, &message_id, input)
            })
            .await
            {
                Ok(view) => response_value(200, view),
                Err(error) => application_value(error),
            }
        }
        TranslationApplicationCall::ClearCache => {
            match run(data_dir, move |store| {
                TranslationService::new(store).clear_cache(&user_id)
            })
            .await
            {
                Ok(cleared) => response_value(200, CacheClearResult { cleared }),
                Err(error) => application_value(error),
            }
        }
    }
}

async fn prepare(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(message_id): Path<String>,
    input: Result<Json<TranslationPreparationRequest>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    respond(
        run(state.config.data_dir.clone(), move |store| {
            TranslationService::new(store).prepare(&user.user_id, &message_id, input)
        })
        .await,
    )
}

async fn clear_cache(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    match run(state.config.data_dir.clone(), move |store| {
        TranslationService::new(store).clear_cache(&user.user_id)
    })
    .await
    {
        Ok(cleared) => Json(CacheClearResult { cleared }).into_response(),
        Err(error) => application_error(error),
    }
}

async fn run<T: Send + 'static>(
    data_dir: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore) -> Result<T, ApplicationError<AuthStoreError>>
        + Send
        + 'static,
) -> Result<T, ApplicationError<AuthStoreError>> {
    match tokio::task::spawn_blocking(move || {
        let mut store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite"))
            .map_err(ApplicationError::Repository)?;
        operation(&mut store)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(ApplicationError::Domain {
            code: "TRANSLATION_WORKER_STOPPED",
            status: 500,
            message: "服务暂时无法完成请求",
        }),
    }
}

fn respond(
    result: Result<TranslationPreparationView, ApplicationError<AuthStoreError>>,
) -> Response {
    match result {
        Ok(view) => Json(view).into_response(),
        Err(error) => application_error(error),
    }
}

fn application_error(cause: ApplicationError<AuthStoreError>) -> Response {
    match cause {
        ApplicationError::Domain {
            status, message, ..
        } if status < 500 => error_response(status, message),
        ApplicationError::Domain { .. } => {
            error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求")
        }
        ApplicationError::Repository(cause) => {
            eprintln!("[imail-http] translation storage error: {cause}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求")
        }
    }
}

fn application_value(error: ApplicationError<AuthStoreError>) -> TranslationApplicationResponse {
    match error {
        ApplicationError::Domain {
            status, message, ..
        } if status < 500 => TranslationApplicationResponse {
            status,
            body: serde_json::json!({"error": message}),
        },
        ApplicationError::Domain { .. } | ApplicationError::Repository(_) => {
            TranslationApplicationResponse {
                status: 500,
                body: serde_json::json!({"error": "服务暂时无法完成请求"}),
            }
        }
    }
}

fn response_value(value_status: u16, value: impl Serialize) -> TranslationApplicationResponse {
    TranslationApplicationResponse {
        status: value_status,
        body: serde_json::to_value(value).expect("translation response serializes"),
    }
}

fn error_response(status: u16, message: &str) -> Response {
    error(
        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
        message,
    )
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({"error": message}))).into_response()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheClearResult {
    cleared: u64,
}
