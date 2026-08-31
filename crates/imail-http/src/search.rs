//! Shared HTTP and embedded desktop smart-folder control plane.
use crate::{auth::AuthenticatedUser, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use imail_core::{search::SmartFolderInput, ApplicationError};
use imail_storage_sqlite::{AuthStoreError, SqliteAuthStore};
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/smart-folders", get(list).post(create))
        .route(
            "/api/smart-folders/:id",
            axum::routing::put(update).delete(remove),
        )
}

pub fn execute(
    store: &mut SqliteAuthStore,
    owner: &str,
    method: &str,
    id: Option<&str>,
    input: Option<Value>,
) -> Result<Value, ApplicationError<AuthStoreError>> {
    let invalid = |message| ApplicationError::Domain {
        code: "SMART_FOLDER_INVALID",
        status: 400,
        message,
    };
    let missing = || ApplicationError::Domain {
        code: "SMART_FOLDER_NOT_FOUND",
        status: 404,
        message: "智能文件夹不存在",
    };
    match method {
        "GET" => Ok(
            json!({"folders": store.list_smart_folders(owner).map_err(ApplicationError::Repository)?}),
        ),
        "POST" | "PUT" => {
            let input: SmartFolderInput =
                serde_json::from_value(input.ok_or_else(|| invalid("缺少查询条件"))?)
                    .map_err(|_| invalid("智能文件夹参数无效"))?;
            input.validate().map_err(invalid)?;
            let folder = store
                .save_smart_folder(owner, id, &input)
                .map_err(|error| match error {
                    AuthStoreError::AccountNotOwned => invalid("查询账户不存在或不属于当前用户"),
                    AuthStoreError::InvalidContentData => {
                        invalid("智能文件夹条件无效或已达到 100 个上限")
                    }
                    other => ApplicationError::Repository(other),
                })?
                .ok_or_else(missing)?;
            Ok(json!({"folder":folder}))
        }
        "DELETE" => {
            if !store
                .delete_smart_folder(owner, id.ok_or_else(missing)?)
                .map_err(ApplicationError::Repository)?
            {
                return Err(missing());
            }
            Ok(json!({"deleted":true}))
        }
        _ => Err(invalid("不支持的操作")),
    }
}

async fn run(
    state: Arc<AppState>,
    owner: String,
    method: &'static str,
    id: Option<String>,
    input: Option<Value>,
) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        let mut store = SqliteAuthStore::open_database(state.config.data_dir.join("imail.sqlite"))
            .map_err(ApplicationError::Repository)?;
        execute(&mut store, &owner, method, id.as_deref(), input)
    })
    .await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(ApplicationError::Domain {
            status, message, ..
        })) => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
            Json(json!({"error":message})),
        )
            .into_response(),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"智能文件夹操作失败"})),
        )
            .into_response(),
    }
}
async fn list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    run(state, user.user_id, "GET", None, None).await
}
async fn create(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(input): Json<Value>,
) -> Response {
    run(state, user.user_id, "POST", None, Some(input)).await
}
async fn update(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    run(state, user.user_id, "PUT", Some(id), Some(input)).await
}
async fn remove(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    run(state, user.user_id, "DELETE", Some(id), None).await
}
