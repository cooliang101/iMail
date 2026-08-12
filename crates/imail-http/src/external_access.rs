use std::sync::Arc;

use axum::{
    extract::State,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use imail_core::{
    external_access::{ExternalAccessChanges, ExternalAccessService, ExternalAccessSettings},
    ApplicationError,
};
use imail_storage_sqlite::SqliteAuthStore;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{auth::AuthenticatedUser, AppState};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/external-access", get(read).patch(update))
}

#[derive(Serialize)]
struct SettingsBody {
    settings: ExternalAccessSettings,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsInput {
    gateway_enabled: Option<bool>,
    mcp_enabled: Option<bool>,
}

async fn read(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        ExternalAccessService::new(store).get(&user.user_id)
    })
    .await
    {
        Ok(settings) => Json(SettingsBody { settings }).into_response(),
        Err(()) => internal_error(),
    }
}

async fn update(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    input: Result<Json<SettingsInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({ "error": "请求参数无效" })),
            )
                .into_response()
        }
    };
    if input.gateway_enabled.is_none() && input.mcp_enabled.is_none() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": "至少提供一个要更新的外部接入设置" })),
        )
            .into_response();
    }
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        ExternalAccessService::new(store).update(
            &user.user_id,
            ExternalAccessChanges {
                gateway_enabled: input.gateway_enabled,
                mcp_enabled: input.mcp_enabled,
            },
        )
    })
    .await
    {
        Ok(settings) => Json(SettingsBody { settings }).into_response(),
        Err(()) => internal_error(),
    }
}

async fn run<T: Send + 'static>(
    database: std::path::PathBuf,
    operation: impl FnOnce(
            &mut SqliteAuthStore,
        ) -> Result<T, ApplicationError<imail_storage_sqlite::AuthStoreError>>
        + Send
        + 'static,
) -> Result<T, ()> {
    tokio::task::spawn_blocking(move || {
        let mut store = SqliteAuthStore::open_database(database).map_err(|_| ())?;
        operation(&mut store).map_err(|_| ())
    })
    .await
    .map_err(|_| ())?
}

fn internal_error() -> Response {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "服务暂时不可用" })),
    )
        .into_response()
}
