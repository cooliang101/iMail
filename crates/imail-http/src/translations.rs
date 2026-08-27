use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, post},
    Extension, Json, Router,
};
use imail_core::{translations::TranslationService, ApplicationError};
use imail_protocol::{
    TranslationArtifact, TranslationCompletionRequest, TranslationCredentialKind,
    TranslationExecutionRequest, TranslationExecutionTarget, TranslationPreparationRequest,
    TranslationPreparationView, TranslationProviderConfiguration,
};
use imail_security::MasterKey;
use imail_storage_sqlite::{AuthStoreError, MasterKeyCredentialCodec, SqliteAuthStore};
use serde::Serialize;

use crate::{
    auth::AuthenticatedUser,
    translation_providers::{DeepLClient, DeepLTranslationRequest, ProviderExecutionError},
    AppState,
};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/messages/:id/translations/prepare", post(prepare))
        .route("/api/messages/:id/translations/run", post(execute))
        .route("/api/messages/:id/translations/complete", post(complete))
        .route("/api/translation-cache", delete(clear_cache))
}

#[derive(Debug)]
pub enum TranslationApplicationCall {
    Prepare {
        message_id: String,
        input: TranslationPreparationRequest,
    },
    Complete {
        message_id: String,
        input: TranslationCompletionRequest,
    },
    Execute {
        message_id: String,
        input: TranslationExecutionRequest,
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
        TranslationApplicationCall::Execute { message_id, input } => {
            match run_with_key(data_dir, move |store, key| {
                execute_network_provider(store, key, &user_id, &message_id, input)
            })
            .await
            {
                Ok(artifact) => response_value(200, artifact),
                Err(error) => application_value(error),
            }
        }
        TranslationApplicationCall::Complete { message_id, input } => {
            match run(data_dir, move |store| {
                let now = chrono::Utc::now().to_rfc3339();
                TranslationService::new(store).complete(&user_id, &message_id, input, &now)
            })
            .await
            {
                Ok(artifact) => response_value(200, artifact),
                Err(error) => application_value(error),
            }
        }
    }
}

async fn execute(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(message_id): Path<String>,
    input: Result<Json<TranslationExecutionRequest>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    respond_artifact(
        run_with_key(state.config.data_dir.clone(), move |store, key| {
            execute_network_provider(store, key, &user.user_id, &message_id, input)
        })
        .await,
    )
}

fn execute_network_provider(
    store: &mut SqliteAuthStore,
    key: &MasterKey,
    user_id: &str,
    message_id: &str,
    input: TranslationExecutionRequest,
) -> Result<TranslationArtifact, ApplicationError<AuthStoreError>> {
    let preparation = TranslationService::new(store).prepare(
        user_id,
        message_id,
        TranslationPreparationRequest {
            profile_id: input.profile_id.clone(),
            source_language: input.source_language.clone(),
            target_language: input.target_language.clone(),
        },
    )?;
    if let Some(cached) = preparation.cached.clone() {
        return Ok(cached);
    }
    if preparation.profile.profile.execution_target == TranslationExecutionTarget::WebView {
        return Err(domain(
            "TRANSLATION_SERVER_EXECUTION_FORBIDDEN",
            403,
            "该翻译服务不能由服务端执行",
        ));
    }
    let plan = match preparation.profile.profile.provider {
        TranslationProviderConfiguration::DeepL { plan } => plan,
        _ => {
            return Err(domain(
                "TRANSLATION_PROVIDER_NOT_IMPLEMENTED",
                501,
                "所选翻译服务尚未接入",
            ))
        }
    };
    let codec = MasterKeyCredentialCodec::new(key);
    let api_key = imail_core::translation_settings::TranslationSettingsService::new(store)
        .credential_secret(
            user_id,
            &input.profile_id,
            TranslationCredentialKind::DeepLApiKey,
            &codec,
        )?;
    let segments = DeepLClient::default()
        .translate(DeepLTranslationRequest {
            api_key: &api_key,
            plan,
            source_language: input.source_language.as_deref(),
            target_language: &input.target_language,
            segments: &preparation.document.segments,
        })
        .map_err(provider_error)?;
    let now = chrono::Utc::now().to_rfc3339();
    TranslationService::new(store).store_artifact(&preparation, segments, &now, &now)
}

fn provider_error(error: ProviderExecutionError) -> ApplicationError<AuthStoreError> {
    domain(error.code(), error.status(), error.message())
}

fn domain(
    code: &'static str,
    status: u16,
    message: &'static str,
) -> ApplicationError<AuthStoreError> {
    ApplicationError::Domain {
        code,
        status,
        message,
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

async fn complete(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(message_id): Path<String>,
    input: Result<Json<TranslationCompletionRequest>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    respond_artifact(
        run(state.config.data_dir.clone(), move |store| {
            let now = chrono::Utc::now().to_rfc3339();
            TranslationService::new(store).complete(&user.user_id, &message_id, input, &now)
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

async fn run_with_key<T: Send + 'static>(
    data_dir: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore, &MasterKey) -> Result<T, ApplicationError<AuthStoreError>>
        + Send
        + 'static,
) -> Result<T, ApplicationError<AuthStoreError>> {
    run(data_dir.clone(), move |store| {
        let key = MasterKey::from_file(data_dir.join("master.key")).map_err(|_| {
            ApplicationError::Domain {
                code: "TRANSLATION_MASTER_KEY_INVALID",
                status: 500,
                message: "无法读取翻译服务安全存储",
            }
        })?;
        operation(store, &key)
    })
    .await
}

fn respond(
    result: Result<TranslationPreparationView, ApplicationError<AuthStoreError>>,
) -> Response {
    match result {
        Ok(view) => Json(view).into_response(),
        Err(error) => application_error(error),
    }
}

fn respond_artifact(
    result: Result<TranslationArtifact, ApplicationError<AuthStoreError>>,
) -> Response {
    match result {
        Ok(artifact) => Json(artifact).into_response(),
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
