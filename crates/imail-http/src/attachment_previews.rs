use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use imail_attachment::{
    normalize_text, read_archive_entry, AttachmentError, PreviewDescriptor, PreviewKind,
};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::AuthenticatedUser;
use crate::{AppState, EmbeddedOperationError};

const PREVIEW_TTL: Duration = Duration::from_secs(20 * 60);
const MAX_CACHE_BYTES: usize = 256 * 1024 * 1024;
const MAX_USER_CACHE_BYTES: usize = 100 * 1024 * 1024;

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/messages/:message_id/attachments/:index/preview",
            post(create),
        )
        .route("/api/attachment-previews/:preview_id", delete(remove))
        .route("/api/attachment-previews/:preview_id/content", get(content))
        .route(
            "/api/attachment-previews/:preview_id/archive/entries/:entry_id",
            get(archive_entry),
        )
}

#[derive(Default)]
pub(crate) struct PreviewState {
    records: Mutex<HashMap<String, PreviewRecord>>,
}

struct PreviewRecord {
    owner_id: String,
    content: Vec<u8>,
    descriptor: PreviewDescriptor,
    created_at: Instant,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewSession {
    preview_id: String,
    descriptor: PreviewDescriptor,
    expires_in_seconds: u64,
}

async fn create(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((message_id, index)): Path<(String, String)>,
) -> Response {
    let Ok(index) = index.parse::<usize>() else {
        return error(StatusCode::BAD_REQUEST, "附件索引无效");
    };
    match create_for(state, user.user_id, message_id, index).await {
        Ok(session) => (StatusCode::CREATED, Json(session)).into_response(),
        Err(CreateError::Mail(cause)) => crate::messages::mail_error(cause),
        Err(CreateError::Attachment(cause)) => attachment_error(cause),
        Err(CreateError::Internal) => error(StatusCode::INTERNAL_SERVER_ERROR, "附件预览处理失败"),
        Err(CreateError::Capacity) => {
            error(StatusCode::INSUFFICIENT_STORAGE, "附件预览缓存空间不足")
        }
    }
}

async fn remove(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(preview_id): Path<String>,
) -> Response {
    match remove_for(&state, &user.user_id, &preview_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(()) => error(StatusCode::NOT_FOUND, "附件预览不存在或已过期"),
    }
}

async fn content(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(preview_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let owner_id = user.user_id;
    let result =
        tokio::task::spawn_blocking(move || resource_for(&state, &owner_id, &preview_id, None))
            .await
            .unwrap_or(Err(ResourceError::Missing));
    match result {
        Ok(resource) => resource_response(resource, headers.get("range")),
        Err(ResourceError::Missing) => error(StatusCode::NOT_FOUND, "附件预览不存在或已过期"),
        Err(ResourceError::Attachment(cause)) => attachment_error(cause),
    }
}

async fn archive_entry(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((preview_id, entry_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let owner_id = user.user_id;
    let result = tokio::task::spawn_blocking(move || {
        resource_for(&state, &owner_id, &preview_id, Some(&entry_id))
    })
    .await
    .unwrap_or(Err(ResourceError::Missing));
    match result {
        Ok(resource) => resource_response(resource, headers.get("range")),
        Err(ResourceError::Missing) => error(StatusCode::NOT_FOUND, "压缩包条目不存在或预览已过期"),
        Err(ResourceError::Attachment(cause)) => attachment_error(cause),
    }
}

enum CreateError {
    Mail(imail_core::mail_operations::MailApplicationError<imail_storage_sqlite::AuthStoreError>),
    Attachment(AttachmentError),
    Capacity,
    Internal,
}

async fn create_for(
    state: Arc<AppState>,
    owner_id: String,
    message_id: String,
    index: usize,
) -> Result<PreviewSession, CreateError> {
    let attachment = crate::messages::download_attachment_for(
        Arc::clone(&state),
        owner_id.clone(),
        message_id,
        index,
    )
    .await
    .map_err(CreateError::Mail)?;
    let filename = attachment.filename;
    let content_type = attachment.content_type;
    let content = attachment.content;
    let (descriptor, content) = tokio::task::spawn_blocking(move || {
        imail_attachment::inspect(&filename, &content_type, &content).and_then(|descriptor| {
            let content = if descriptor.kind == PreviewKind::Text {
                normalize_text(&content)?
            } else {
                content
            };
            Ok((descriptor, content))
        })
    })
    .await
    .map_err(|_| CreateError::Internal)?
    .map_err(CreateError::Attachment)?;
    let preview_id = Uuid::new_v4().simple().to_string();
    let record = PreviewRecord {
        owner_id: owner_id.clone(),
        content,
        descriptor: descriptor.clone(),
        created_at: Instant::now(),
    };
    let mut records = state
        .attachment_previews
        .records
        .lock()
        .map_err(|_| CreateError::Capacity)?;
    cleanup(&mut records);
    let global_bytes = records
        .values()
        .map(|item| item.content.len())
        .sum::<usize>();
    let user_bytes = records
        .values()
        .filter(|item| item.owner_id == owner_id)
        .map(|item| item.content.len())
        .sum::<usize>();
    if global_bytes.saturating_add(record.content.len()) > MAX_CACHE_BYTES
        || user_bytes.saturating_add(record.content.len()) > MAX_USER_CACHE_BYTES
    {
        return Err(CreateError::Capacity);
    }
    records.insert(preview_id.clone(), record);
    Ok(PreviewSession {
        preview_id,
        descriptor,
        expires_in_seconds: PREVIEW_TTL.as_secs(),
    })
}

fn cleanup(records: &mut HashMap<String, PreviewRecord>) {
    records.retain(|_, record| record.created_at.elapsed() < PREVIEW_TTL);
}

fn remove_for(state: &AppState, owner_id: &str, preview_id: &str) -> Result<(), ()> {
    let mut records = state.attachment_previews.records.lock().map_err(|_| ())?;
    cleanup(&mut records);
    if records
        .get(preview_id)
        .is_some_and(|record| record.owner_id == owner_id)
    {
        records.remove(preview_id);
        Ok(())
    } else {
        Err(())
    }
}

struct Resource {
    bytes: Vec<u8>,
    filename: String,
    content_type: String,
}

enum ResourceError {
    Missing,
    Attachment(AttachmentError),
}

fn resource_for(
    state: &AppState,
    owner_id: &str,
    preview_id: &str,
    entry_id: Option<&str>,
) -> Result<Resource, ResourceError> {
    let mut records = state
        .attachment_previews
        .records
        .lock()
        .map_err(|_| ResourceError::Missing)?;
    cleanup(&mut records);
    let (content, descriptor) = {
        let record = records
            .get(preview_id)
            .filter(|record| record.owner_id == owner_id)
            .ok_or(ResourceError::Missing)?;
        (record.content.clone(), record.descriptor.clone())
    };
    drop(records);
    if let Some(entry_id) = entry_id {
        if descriptor.kind != PreviewKind::Archive {
            return Err(ResourceError::Missing);
        }
        let (entry, bytes) =
            read_archive_entry(&content, entry_id).map_err(ResourceError::Attachment)?;
        return Ok(Resource {
            bytes,
            filename: entry.name,
            content_type: entry.content_type,
        });
    }
    Ok(Resource {
        bytes: content,
        filename: descriptor.filename,
        content_type: descriptor.content_type,
    })
}

fn resource_response(resource: Resource, range: Option<&HeaderValue>) -> Response {
    let total = resource.bytes.len();
    let range = range
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_range(value, total));
    let (status, bytes, content_range) = match range {
        Some((start, end)) => (
            StatusCode::PARTIAL_CONTENT,
            resource.bytes[start..=end].to_vec(),
            Some(format!("bytes {start}-{end}/{total}")),
        ),
        None => (StatusCode::OK, resource.bytes, None),
    };
    let filename = resource.filename.replace(['\r', '\n', '"', '\\'], "_");
    let encoded: String = url::form_urlencoded::byte_serialize(filename.as_bytes()).collect();
    let content_length = bytes.len();
    let mut response = (status, bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&resource.content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("inline; filename*=UTF-8''{encoded}"))
            .unwrap_or_else(|_| HeaderValue::from_static("inline")),
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&content_length.to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("0")),
    );
    if let Some(value) = content_range.and_then(|value| HeaderValue::from_str(&value).ok()) {
        headers.insert(CONTENT_RANGE, value);
    }
    response
}

fn parse_range(value: &str, total: usize) -> Option<(usize, usize)> {
    if total == 0 || !value.starts_with("bytes=") || value.contains(',') {
        return None;
    }
    let (start, end) = value[6..].split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<usize>().ok()?.min(total);
        return (suffix > 0).then_some((total - suffix, total - 1));
    }
    let start = start.parse::<usize>().ok()?;
    if start >= total {
        return None;
    }
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<usize>().ok()?.min(total - 1)
    };
    (start <= end).then_some((start, end))
}

fn attachment_error(cause: AttachmentError) -> Response {
    let status = match cause {
        AttachmentError::ArchiveEntryMissing => StatusCode::NOT_FOUND,
        AttachmentError::TooLarge
        | AttachmentError::ImageDimensionsTooLarge
        | AttachmentError::TextTooLarge
        | AttachmentError::TooManyArchiveEntries
        | AttachmentError::ArchiveExpandedTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    error(status, cause.to_string())
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

pub(crate) async fn embedded_create(
    state: Arc<AppState>,
    owner_id: String,
    message_id: String,
    index: usize,
) -> Result<serde_json::Value, EmbeddedOperationError> {
    create_for(state, owner_id, message_id, index)
        .await
        .map(|value| serde_json::to_value(value).expect("preview session serializes"))
        .map_err(|cause| match cause {
            CreateError::Mail(error) => EmbeddedOperationError {
                status: 422,
                message: error.to_string(),
            },
            CreateError::Attachment(error) => EmbeddedOperationError {
                status: 422,
                message: error.to_string(),
            },
            CreateError::Capacity => EmbeddedOperationError {
                status: 507,
                message: "附件预览缓存空间不足".into(),
            },
            CreateError::Internal => EmbeddedOperationError {
                status: 500,
                message: "附件预览处理失败".into(),
            },
        })
}

pub(crate) fn embedded_content(
    state: &AppState,
    owner_id: &str,
    preview_id: &str,
    entry_id: Option<&str>,
) -> Result<Vec<u8>, EmbeddedOperationError> {
    resource_for(state, owner_id, preview_id, entry_id)
        .map(|resource| resource.bytes)
        .map_err(|cause| EmbeddedOperationError {
            status: 404,
            message: match cause {
                ResourceError::Missing => "附件预览不存在或已过期".into(),
                ResourceError::Attachment(error) => error.to_string(),
            },
        })
}

pub(crate) fn embedded_delete(
    state: &AppState,
    owner_id: &str,
    preview_id: &str,
) -> Result<(), EmbeddedOperationError> {
    remove_for(state, owner_id, preview_id).map_err(|_| EmbeddedOperationError {
        status: 404,
        message: "附件预览不存在或已过期".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_byte_ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Some((0, 9)));
        assert_eq!(parse_range("bytes=90-", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
    }
}
