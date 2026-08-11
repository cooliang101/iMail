use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{rejection::JsonRejection, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use imail_core::{preferences::PreferencesService, ApplicationError};
use imail_protocol::{AppPreferences, AppPreferencesPatch};
use imail_storage_sqlite::{AuthStoreError, SqliteAuthStore};
use serde::Serialize;

use crate::{auth::AuthenticatedUser, AppState};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/preferences", get(read).patch(update))
}

#[derive(Serialize)]
struct PreferencesBody {
    preferences: AppPreferences,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

async fn read(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        PreferencesService::new(store).read(&user.user_id)
    })
    .await
    {
        Ok(preferences) => Json(PreferencesBody { preferences }).into_response(),
        Err(error) => application_error(error),
    }
}

async fn update(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    patch: Result<Json<AppPreferencesPatch>, JsonRejection>,
) -> Response {
    let Json(patch) = match patch {
        Ok(patch) => patch,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        PreferencesService::new(store).update(&user.user_id, patch)
    })
    .await
    {
        Ok(preferences) => Json(PreferencesBody { preferences }).into_response(),
        Err(error) => application_error(error),
    }
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
            eprintln!("[imail-http] preferences storage error: {storage_error}");
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
