use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use imail_mail::{DownloadedAttachment, MAX_RFC822_BYTES};
use imail_protocol::MessageReadModel;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const CACHE_SCHEMA_VERSION: u8 = 1;
const CACHE_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MAX_USER_CACHE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 16 * 1024;

pub(crate) struct AttachmentCacheKey {
    owner: String,
    entry: String,
}

impl AttachmentCacheKey {
    pub(crate) fn new(owner_id: &str, message: &MessageReadModel, index: usize) -> Self {
        let owner = digest(owner_id.as_bytes());
        let mut identity = Sha256::new();
        let uid = message.uid.to_string();
        let index = index.to_string();
        for value in [
            message.id.as_bytes(),
            message.account_id.as_bytes(),
            message.mailbox.as_bytes(),
            uid.as_bytes(),
            message.message_id.as_deref().unwrap_or_default().as_bytes(),
            index.as_bytes(),
        ] {
            identity.update((value.len() as u64).to_be_bytes());
            identity.update(value);
        }
        if let Ok(attachments) = serde_json::to_vec(&message.attachments) {
            identity.update((attachments.len() as u64).to_be_bytes());
            identity.update(attachments);
        }
        Self {
            owner,
            entry: format!("{:x}", identity.finalize()),
        }
    }

    fn directory(&self, data_dir: &Path) -> PathBuf {
        data_dir.join("attachment-cache").join(&self.owner)
    }

    fn content_path(&self, data_dir: &Path) -> PathBuf {
        self.directory(data_dir).join(format!("{}.bin", self.entry))
    }

    fn metadata_path(&self, data_dir: &Path) -> PathBuf {
        self.directory(data_dir)
            .join(format!("{}.json", self.entry))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheMetadata {
    schema_version: u8,
    filename: String,
    content_type: String,
    size: usize,
    sha256: String,
}

pub(crate) fn read(data_dir: &Path, key: &AttachmentCacheKey) -> Option<DownloadedAttachment> {
    let metadata_path = key.metadata_path(data_dir);
    let content_path = key.content_path(data_dir);
    let result = read_valid(&metadata_path, &content_path);
    if result.is_none() && (metadata_path.exists() || content_path.exists()) {
        let _ = fs::remove_file(metadata_path);
        let _ = fs::remove_file(content_path);
    }
    result
}

fn read_valid(metadata_path: &Path, content_path: &Path) -> Option<DownloadedAttachment> {
    let metadata_info = fs::metadata(metadata_path).ok()?;
    if metadata_info.len() > MAX_METADATA_BYTES || expired(&metadata_info) {
        return None;
    }
    let content_info = fs::metadata(content_path).ok()?;
    if content_info.len() > MAX_RFC822_BYTES as u64 || expired(&content_info) {
        return None;
    }
    let metadata = serde_json::from_slice::<CacheMetadata>(&fs::read(metadata_path).ok()?).ok()?;
    if metadata.schema_version != CACHE_SCHEMA_VERSION || metadata.size as u64 != content_info.len()
    {
        return None;
    }
    let content = fs::read(content_path).ok()?;
    if digest(&content) != metadata.sha256 {
        return None;
    }
    Some(DownloadedAttachment {
        content,
        filename: metadata.filename,
        content_type: metadata.content_type,
    })
}

pub(crate) fn write(
    data_dir: &Path,
    key: &AttachmentCacheKey,
    attachment: &DownloadedAttachment,
) -> io::Result<()> {
    if attachment.content.len() > MAX_RFC822_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "attachment cache item is too large",
        ));
    }
    let directory = key.directory(data_dir);
    fs::create_dir_all(&directory)?;
    private_directory(&directory)?;
    cleanup(&directory, attachment.content.len() as u64);

    let metadata = CacheMetadata {
        schema_version: CACHE_SCHEMA_VERSION,
        filename: attachment.filename.clone(),
        content_type: attachment.content_type.clone(),
        size: attachment.content.len(),
        sha256: digest(&attachment.content),
    };
    let metadata = serde_json::to_vec(&metadata)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let suffix = Uuid::new_v4().simple().to_string();
    let content_temporary = directory.join(format!(".{}-{suffix}.bin", key.entry));
    let metadata_temporary = directory.join(format!(".{}-{suffix}.json", key.entry));
    fs::write(&content_temporary, &attachment.content)?;
    fs::write(&metadata_temporary, metadata)?;
    private_file(&content_temporary)?;
    private_file(&metadata_temporary)?;
    commit(&content_temporary, &key.content_path(data_dir))?;
    commit(&metadata_temporary, &key.metadata_path(data_dir))?;
    Ok(())
}

pub(crate) fn clear_user(data_dir: &Path, owner_id: &str) -> io::Result<()> {
    let directory = data_dir
        .join("attachment-cache")
        .join(digest(owner_id.as_bytes()));
    match fs::remove_dir_all(directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn cleanup(directory: &Path, incoming: u64) {
    let now = SystemTime::now();
    let mut entries = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("bin") {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            Some((path, metadata.len(), modified))
        })
        .collect::<Vec<_>>();
    for (path, _, _) in entries
        .iter()
        .filter(|(_, _, modified)| now.duration_since(*modified).unwrap_or_default() > CACHE_TTL)
    {
        remove_pair(path);
    }
    entries.retain(|(path, _, modified)| {
        path.exists() && now.duration_since(*modified).unwrap_or_default() <= CACHE_TTL
    });
    entries.sort_by_key(|(_, _, modified)| *modified);
    let mut total = entries.iter().map(|(_, size, _)| *size).sum::<u64>();
    for (path, size, _) in entries {
        if total.saturating_add(incoming) <= MAX_USER_CACHE_BYTES {
            break;
        }
        remove_pair(&path);
        total = total.saturating_sub(size);
    }
}

fn remove_pair(content_path: &Path) {
    let _ = fs::remove_file(content_path);
    let _ = fs::remove_file(content_path.with_extension("json"));
}

fn expired(metadata: &fs::Metadata) -> bool {
    metadata
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age > CACHE_TTL)
}

fn commit(temporary: &Path, target: &Path) -> io::Result<()> {
    match fs::rename(temporary, target) {
        Ok(()) => Ok(()),
        Err(_) if target.exists() => {
            let _ = fs::remove_file(temporary);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(temporary);
            Err(error)
        }
    }
}

fn digest(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

#[cfg(unix)]
fn private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn private_directory(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn private_file(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn private_file(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(owner_variant: &str) -> MessageReadModel {
        MessageReadModel {
            id: format!("message-{owner_variant}"),
            account_id: "account".into(),
            mailbox: "INBOX".into(),
            mailbox_role: "inbox".into(),
            uid: 42,
            message_id: Some("<cached@example.org>".into()),
            from: json!({}),
            to: json!([]),
            subject: "Cached".into(),
            preview: String::new(),
            text: String::new(),
            html: None,
            date: "2026-08-12T00:00:00Z".into(),
            unread: false,
            flagged: false,
            has_attachments: true,
            attachments: json!([{"filename":"report.txt","contentType":"text/plain","size":5,"index":0}]),
            labels: json!([]),
            snoozed_until: None,
        }
    }

    #[test]
    fn round_trips_and_isolates_cached_attachments() {
        let directory =
            std::env::temp_dir().join(format!("imail-attachment-cache-{}", Uuid::new_v4()));
        let first = AttachmentCacheKey::new("owner-a", &message("same"), 0);
        let other = AttachmentCacheKey::new("owner-b", &message("same"), 0);
        let attachment = DownloadedAttachment {
            content: b"hello".to_vec(),
            filename: "report.txt".into(),
            content_type: "text/plain".into(),
        };
        write(&directory, &first, &attachment).unwrap();
        assert_eq!(read(&directory, &first), Some(attachment));
        assert_eq!(read(&directory, &other), None);
        clear_user(&directory, "owner-a").unwrap();
        assert_eq!(read(&directory, &first), None);
        fs::remove_dir_all(directory).unwrap();
    }
}
