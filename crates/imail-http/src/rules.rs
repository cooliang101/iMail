//! Shared session-authenticated HTTP / embedded desktop / full-scope MCP API.
use crate::{auth::AuthenticatedUser, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use imail_core::{rules::MailRuleInput, ApplicationError};
use imail_storage_sqlite::{AuthStoreError, SqliteAuthStore};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/mail-rules", get(list).post(create))
        .route(
            "/api/mail-rules/:id",
            axum::routing::put(update).delete(remove),
        )
        .route("/api/mail-rules/preview", post(preview))
        .route("/api/mail-rules/apply", post(apply))
        .route("/api/mail-rule-runs", get(runs))
        .route("/api/mail-rule-runs/:id/retry", post(retry))
}

fn invalid(message: &'static str) -> ApplicationError<AuthStoreError> {
    ApplicationError::Domain {
        code: "MAIL_RULE_INVALID",
        status: 400,
        message,
    }
}
fn missing() -> ApplicationError<AuthStoreError> {
    ApplicationError::Domain {
        code: "MAIL_RULE_NOT_FOUND",
        status: 404,
        message: "规则或执行记录不存在",
    }
}
fn storage(error: AuthStoreError) -> ApplicationError<AuthStoreError> {
    match error {
        AuthStoreError::InvalidContentData => {
            invalid("规则参数或预览已失效，请重新预览；规则最多 100 条")
        }
        AuthStoreError::AccountNotOwned => invalid("所选邮箱不存在或不属于当前用户"),
        other => ApplicationError::Repository(other),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewInput {
    input: MailRuleInput,
    rule_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyInput {
    token: String,
    confirmed: bool,
}

pub fn execute(
    store: &mut SqliteAuthStore,
    owner: &str,
    operation: &str,
    id: Option<&str>,
    input: Option<Value>,
) -> Result<Value, ApplicationError<AuthStoreError>> {
    match operation {
        "list" => Ok(json!({"rules":store.list_mail_rules(owner).map_err(storage)?})),
        "create" | "update" => {
            let input: MailRuleInput =
                serde_json::from_value(input.ok_or_else(|| invalid("缺少规则参数"))?)
                    .map_err(|_| invalid("规则参数无效"))?;
            input.validate().map_err(invalid)?;
            if operation == "update" && id.is_none() {
                return Err(missing());
            }
            Ok(
                json!({"rule":store.save_mail_rule(owner,id,&input).map_err(storage)?.ok_or_else(missing)?}),
            )
        }
        "delete" => {
            if !store
                .delete_mail_rule(owner, id.ok_or_else(missing)?)
                .map_err(storage)?
            {
                return Err(missing());
            }
            Ok(json!({"deleted":true}))
        }
        "preview" => {
            let input: PreviewInput =
                serde_json::from_value(input.ok_or_else(|| invalid("缺少预览参数"))?)
                    .map_err(|_| invalid("预览参数无效"))?;
            input.input.validate().map_err(invalid)?;
            Ok(json!(store
                .preview_mail_rule(owner, &input.input, input.rule_id.as_deref())
                .map_err(storage)?))
        }
        "apply" => {
            let input: ApplyInput =
                serde_json::from_value(input.ok_or_else(|| invalid("缺少执行确认"))?)
                    .map_err(|_| invalid("执行参数无效"))?;
            if !input.confirmed {
                return Err(invalid("执行历史邮件规则需要单独确认"));
            }
            Ok(
                json!({"queued":store.apply_mail_rule_preview(owner,&input.token).map_err(storage)?}),
            )
        }
        "runs" => Ok(json!({"runs":store.mail_rule_runs(owner).map_err(storage)?})),
        "retry" => {
            if !store
                .retry_mail_rule_run(owner, id.ok_or_else(missing)?)
                .map_err(storage)?
            {
                return Err(missing());
            }
            Ok(json!({"queued":true}))
        }
        _ => Err(invalid("不支持的规则操作")),
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
            .map_err(storage)?;
        execute(&mut store, &owner, operation, id.as_deref(), input)
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
            Json(json!({"error":"邮件规则操作失败"})),
        )
            .into_response(),
    }
}
async fn list(
    State(s): State<Arc<AppState>>,
    Extension(u): Extension<AuthenticatedUser>,
) -> Response {
    run(s, u.user_id, "list", None, None).await
}
async fn runs(
    State(s): State<Arc<AppState>>,
    Extension(u): Extension<AuthenticatedUser>,
) -> Response {
    run(s, u.user_id, "runs", None, None).await
}
async fn create(
    State(s): State<Arc<AppState>>,
    Extension(u): Extension<AuthenticatedUser>,
    Json(v): Json<Value>,
) -> Response {
    run(s, u.user_id, "create", None, Some(v)).await
}
async fn update(
    State(s): State<Arc<AppState>>,
    Extension(u): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(v): Json<Value>,
) -> Response {
    run(s, u.user_id, "update", Some(id), Some(v)).await
}
async fn remove(
    State(s): State<Arc<AppState>>,
    Extension(u): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    run(s, u.user_id, "delete", Some(id), None).await
}
async fn preview(
    State(s): State<Arc<AppState>>,
    Extension(u): Extension<AuthenticatedUser>,
    Json(v): Json<Value>,
) -> Response {
    run(s, u.user_id, "preview", None, Some(v)).await
}
async fn apply(
    State(s): State<Arc<AppState>>,
    Extension(u): Extension<AuthenticatedUser>,
    Json(v): Json<Value>,
) -> Response {
    run(s, u.user_id, "apply", None, Some(v)).await
}
async fn retry(
    State(s): State<Arc<AppState>>,
    Extension(u): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    run(s, u.user_id, "retry", Some(id), None).await
}
