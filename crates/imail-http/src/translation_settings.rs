use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Extension, Json, Router,
};
use chrono::{SecondsFormat, Utc};
use imail_core::{
    translation_settings::{TranslationCredentialSecret, TranslationSettingsService},
    ApplicationError,
};
use imail_protocol::{
    TranslationCredentialKind, TranslationProviderProfileInput, TranslationSettingsUpdate,
    TranslationSettingsView,
};
use imail_security::MasterKey;
use imail_storage_sqlite::{AuthStoreError, MasterKeyCredentialCodec, SqliteAuthStore};
use serde::{Deserialize, Serialize};

use crate::{auth::AuthenticatedUser, AppState};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/translation-settings", get(read).put(update))
        .route(
            "/api/translation-profiles/:id",
            put(upsert_profile).delete(remove_profile),
        )
        .route(
            "/api/translation-profiles/:id/credential",
            put(update_credential).delete(clear_credential),
        )
        .route(
            "/api/translation-profiles/:id/consent",
            post(accept_consent).delete(revoke_consent),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationCredentialInput {
    pub kind: TranslationCredentialKind,
    pub secret: String,
}

#[derive(Debug)]
pub enum TranslationSettingsApplicationCall {
    Read,
    Update(TranslationSettingsUpdate),
    UpsertProfile {
        profile_id: String,
        input: TranslationProviderProfileInput,
    },
    UpdateCredential {
        profile_id: String,
        input: TranslationCredentialInput,
    },
    ClearCredential {
        profile_id: String,
    },
    AcceptConsent {
        profile_id: String,
    },
    RevokeConsent {
        profile_id: String,
    },
    DeleteProfile {
        profile_id: String,
    },
}

#[derive(Debug)]
pub struct TranslationSettingsApplicationResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

pub async fn embedded_call(
    data_dir: PathBuf,
    user_id: String,
    call: TranslationSettingsApplicationCall,
) -> TranslationSettingsApplicationResponse {
    let result = run(data_dir, move |store, key| {
        let mut service = TranslationSettingsService::new(store);
        match call {
            TranslationSettingsApplicationCall::Read => service.read(&user_id),
            TranslationSettingsApplicationCall::Update(update) => service.update(&user_id, update),
            TranslationSettingsApplicationCall::UpsertProfile { profile_id, input } => {
                service.upsert_profile(&user_id, &profile_id, input, &now())
            }
            TranslationSettingsApplicationCall::UpdateCredential { profile_id, input } => {
                let codec = MasterKeyCredentialCodec::new(key);
                service.set_credential(
                    &user_id,
                    &profile_id,
                    TranslationCredentialSecret {
                        kind: input.kind,
                        secret: input.secret,
                    },
                    &codec,
                    &now(),
                )
            }
            TranslationSettingsApplicationCall::ClearCredential { profile_id } => {
                service.clear_credential(&user_id, &profile_id, &now())
            }
            TranslationSettingsApplicationCall::AcceptConsent { profile_id } => {
                service.accept_consent(&user_id, &profile_id, &now())
            }
            TranslationSettingsApplicationCall::RevokeConsent { profile_id } => {
                service.revoke_consent(&user_id, &profile_id, &now())
            }
            TranslationSettingsApplicationCall::DeleteProfile { profile_id } => {
                service.delete_profile(&user_id, &profile_id)
            }
        }
    })
    .await;
    match result {
        Ok(settings) => TranslationSettingsApplicationResponse {
            status: 200,
            body: serde_json::to_value(settings).expect("translation settings serialize"),
        },
        Err(error) => {
            let status = match &error {
                ApplicationError::Domain { status, .. } => *status,
                ApplicationError::Repository(_) => 500,
            };
            let message = match error {
                ApplicationError::Domain { message, .. } if status < 500 => message,
                ApplicationError::Domain { .. } | ApplicationError::Repository(_) => {
                    "服务暂时无法完成请求"
                }
            };
            TranslationSettingsApplicationResponse {
                status,
                body: serde_json::json!({"error": message}),
            }
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

async fn read(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    respond(
        run(state.config.data_dir.clone(), move |store, _| {
            TranslationSettingsService::new(store).read(&user.user_id)
        })
        .await,
    )
}

async fn update(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    input: Result<Json<TranslationSettingsUpdate>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    respond(
        run(state.config.data_dir.clone(), move |store, _| {
            TranslationSettingsService::new(store).update(&user.user_id, input)
        })
        .await,
    )
}

async fn upsert_profile(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(profile_id): Path<String>,
    input: Result<Json<TranslationProviderProfileInput>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    respond(
        run(state.config.data_dir.clone(), move |store, _| {
            TranslationSettingsService::new(store).upsert_profile(
                &user.user_id,
                &profile_id,
                input,
                &now(),
            )
        })
        .await,
    )
}

async fn update_credential(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(profile_id): Path<String>,
    input: Result<Json<TranslationCredentialInput>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    respond(
        run(state.config.data_dir.clone(), move |store, key| {
            let codec = MasterKeyCredentialCodec::new(key);
            TranslationSettingsService::new(store).set_credential(
                &user.user_id,
                &profile_id,
                TranslationCredentialSecret {
                    kind: input.kind,
                    secret: input.secret,
                },
                &codec,
                &now(),
            )
        })
        .await,
    )
}

async fn clear_credential(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(profile_id): Path<String>,
) -> Response {
    respond(
        run(state.config.data_dir.clone(), move |store, _| {
            TranslationSettingsService::new(store).clear_credential(
                &user.user_id,
                &profile_id,
                &now(),
            )
        })
        .await,
    )
}

async fn accept_consent(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(profile_id): Path<String>,
) -> Response {
    respond(
        run(state.config.data_dir.clone(), move |store, _| {
            TranslationSettingsService::new(store).accept_consent(
                &user.user_id,
                &profile_id,
                &now(),
            )
        })
        .await,
    )
}

async fn revoke_consent(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(profile_id): Path<String>,
) -> Response {
    respond(
        run(state.config.data_dir.clone(), move |store, _| {
            TranslationSettingsService::new(store).revoke_consent(
                &user.user_id,
                &profile_id,
                &now(),
            )
        })
        .await,
    )
}

async fn remove_profile(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(profile_id): Path<String>,
) -> Response {
    respond(
        run(state.config.data_dir.clone(), move |store, _| {
            TranslationSettingsService::new(store).delete_profile(&user.user_id, &profile_id)
        })
        .await,
    )
}

async fn run(
    data_dir: PathBuf,
    operation: impl FnOnce(
            &mut SqliteAuthStore,
            &MasterKey,
        ) -> Result<TranslationSettingsView, ApplicationError<AuthStoreError>>
        + Send
        + 'static,
) -> Result<TranslationSettingsView, ApplicationError<AuthStoreError>> {
    match tokio::task::spawn_blocking(move || {
        let key = MasterKey::from_file(data_dir.join("master.key")).map_err(|_| {
            ApplicationError::Domain {
                code: "TRANSLATION_MASTER_KEY_INVALID",
                status: 500,
                message: "无法读取翻译服务安全存储",
            }
        })?;
        let mut store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite"))
            .map_err(ApplicationError::Repository)?;
        operation(&mut store, &key)
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

fn respond(result: Result<TranslationSettingsView, ApplicationError<AuthStoreError>>) -> Response {
    match result {
        Ok(settings) => Json(settings).into_response(),
        Err(cause) => application_error(cause),
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
            eprintln!("[imail-http] translation settings storage error: {storage_error}");
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

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
