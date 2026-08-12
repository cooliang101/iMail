use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use imail_core::{
    developer_tokens::{CreateDeveloperTokenInput, DeveloperTokenService},
    ApplicationError,
};
use imail_storage_sqlite::{AuthStoreError, SqliteAuthStore};
use serde_json::json;

use crate::{auth::AuthenticatedUser, AppState};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/developer-tokens", get(list).post(create))
        .route("/api/developer-tokens/:id", axum::routing::delete(revoke))
}

async fn list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        DeveloperTokenService::new(store).list(&user.user_id)
    })
    .await
    {
        Ok(tokens) => Json(json!({"tokens": tokens})).into_response(),
        Err(_) => internal_error(),
    }
}

async fn create(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    input: Result<Json<CreateDeveloperTokenInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return invalid("请求参数无效"),
    };
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        DeveloperTokenService::new(store).create(&user.user_id, &user.actor, input)
    })
    .await
    {
        Ok(issued) => (StatusCode::CREATED, Json(issued)).into_response(),
        Err(ApplicationError::Domain {
            status, message, ..
        }) if status < 500 => invalid(message),
        Err(_) => internal_error(),
    }
}

async fn revoke(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(token_id): Path<String>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        DeveloperTokenService::new(store).revoke(&user.user_id, &user.actor, &token_id)
    })
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => internal_error(),
    }
}

async fn run<T: Send + 'static>(
    database: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore) -> Result<T, ApplicationError<AuthStoreError>>
        + Send
        + 'static,
) -> Result<T, ApplicationError<AuthStoreError>> {
    tokio::task::spawn_blocking(move || {
        let mut store =
            SqliteAuthStore::open_database(database).map_err(ApplicationError::Repository)?;
        operation(&mut store)
    })
    .await
    .map_err(|_| ApplicationError::Domain {
        code: "DEVELOPER_TOKEN_WORKER_STOPPED",
        status: 500,
        message: "服务暂时不可用",
    })?
}

fn invalid(message: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response()
}

fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "服务暂时不可用" })),
    )
        .into_response()
}
