use std::io::{Cursor, Read};
use std::path::{Component, Path};

use serde::Serialize;

pub const MAX_PREVIEW_BYTES: usize = 50 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 1_000;
pub const MAX_ARCHIVE_ENTRY_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_ARCHIVE_TOTAL_BYTES: u64 = 200 * 1024 * 1024;
pub const MAX_ARCHIVE_RATIO: u64 = 100;
pub const MAX_IMAGE_SIDE: usize = 20_000;
pub const MAX_IMAGE_PIXELS: usize = 40_000_000;
pub const MAX_TEXT_PREVIEW_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewKind {
    Image,
    Pdf,
    Video,
    Archive,
    Text,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    pub id: String,
    pub path: String,
    pub name: String,
    pub size: u64,
    pub directory: bool,
    pub encrypted: bool,
    pub kind: PreviewKind,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDescriptor {
    pub kind: PreviewKind,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
    pub archive_entries: Vec<ArchiveEntry>,
    pub reason: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AttachmentError {
    #[error("附件超过预览大小限制")]
    TooLarge,
    #[error("压缩包格式无效")]
    InvalidArchive,
    #[error("图片数据无效")]
    InvalidImage,
    #[error("图片尺寸超过预览限制")]
    ImageDimensionsTooLarge,
    #[error("文本文件超过预览大小限制")]
    TextTooLarge,
    #[error("文本文件编码无效或包含二进制内容")]
    InvalidText,
    #[error("压缩包条目过多")]
    TooManyArchiveEntries,
    #[error("压缩包解压后过大")]
    ArchiveExpandedTooLarge,
    #[error("压缩包包含不安全路径")]
    UnsafeArchivePath,
    #[error("压缩包条目不存在")]
    ArchiveEntryMissing,
    #[error("压缩包条目不允许读取")]
    ArchiveEntryRejected,
    #[error("无法读取压缩包条目")]
    ArchiveRead,
}

pub fn inspect(
    filename: &str,
    declared_content_type: &str,
    content: &[u8],
) -> Result<PreviewDescriptor, AttachmentError> {
    if content.len() > MAX_PREVIEW_BYTES {
        return Err(AttachmentError::TooLarge);
    }
    let (kind, content_type) = detect(filename, declared_content_type, content);
    if kind == PreviewKind::Image {
        let dimensions =
            imagesize::blob_size(content).map_err(|_| AttachmentError::InvalidImage)?;
        if dimensions.width > MAX_IMAGE_SIDE
            || dimensions.height > MAX_IMAGE_SIDE
            || dimensions.width.saturating_mul(dimensions.height) > MAX_IMAGE_PIXELS
        {
            return Err(AttachmentError::ImageDimensionsTooLarge);
        }
    }
    if kind == PreviewKind::Text {
        normalize_text(content)?;
    }
    let archive_entries = if kind == PreviewKind::Archive {
        archive_entries(content)?
    } else {
        Vec::new()
    };
    let reason =
        (kind == PreviewKind::Unsupported).then(|| "当前附件类型暂不支持应用内查看".to_string());
    Ok(PreviewDescriptor {
        kind,
        filename: filename.to_string(),
        content_type,
        size: content.len(),
        archive_entries,
        reason,
    })
}

pub fn detect(
    filename: &str,
    declared_content_type: &str,
    content: &[u8],
) -> (PreviewKind, String) {
    let detected = infer::get(content).map(|value| value.mime_type());
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mime = detected
        .unwrap_or(declared_content_type)
        .to_ascii_lowercase();

    if content.is_empty() {
        let hinted = match extension.as_str() {
            "jpg" | "jpeg" => Some((PreviewKind::Image, "image/jpeg")),
            "png" => Some((PreviewKind::Image, "image/png")),
            "gif" => Some((PreviewKind::Image, "image/gif")),
            "webp" => Some((PreviewKind::Image, "image/webp")),
            "bmp" => Some((PreviewKind::Image, "image/bmp")),
            "pdf" => Some((PreviewKind::Pdf, "application/pdf")),
            "mp4" => Some((PreviewKind::Video, "video/mp4")),
            "webm" => Some((PreviewKind::Video, "video/webm")),
            "ogv" | "ogg" => Some((PreviewKind::Video, "video/ogg")),
            extension if is_text_extension(extension) => {
                Some((PreviewKind::Text, "text/plain; charset=utf-8"))
            }
            _ => None,
        };
        if let Some((kind, content_type)) = hinted {
            return (kind, content_type.to_string());
        }
    }

    if matches!(
        mime.as_str(),
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/bmp"
    ) {
        return (PreviewKind::Image, mime);
    }
    if (mime == "application/pdf" || extension == "pdf") && content.starts_with(b"%PDF-") {
        return (PreviewKind::Pdf, "application/pdf".into());
    }
    if matches!(detected, Some("video/mp4" | "video/webm" | "video/ogg")) {
        return (
            PreviewKind::Video,
            detected.unwrap_or("video/mp4").to_string(),
        );
    }
    if (extension == "ogv" || mime == "video/ogg") && content.starts_with(b"OggS") {
        return (PreviewKind::Video, "video/ogg".into());
    }
    if (mime == "application/zip" || extension == "zip")
        && (content.starts_with(b"PK\x03\x04")
            || content.starts_with(b"PK\x05\x06")
            || content.starts_with(b"PK\x07\x08"))
    {
        return (PreviewKind::Archive, "application/zip".into());
    }
    if (is_text_content_type(&mime) || is_text_extension(&extension))
        && normalize_text(content).is_ok()
    {
        return (PreviewKind::Text, "text/plain; charset=utf-8".into());
    }
    (PreviewKind::Unsupported, "application/octet-stream".into())
}

pub fn normalize_text(content: &[u8]) -> Result<Vec<u8>, AttachmentError> {
    if content.len() > MAX_TEXT_PREVIEW_BYTES {
        return Err(AttachmentError::TextTooLarge);
    }
    let decoded = if let Some(value) = content.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        std::str::from_utf8(value).ok().map(str::to_owned)
    } else if let Some(value) = content.strip_prefix(&[0xff, 0xfe]) {
        encoding_rs::UTF_16LE
            .decode_without_bom_handling_and_without_replacement(value)
            .map(|value| value.into_owned())
    } else if let Some(value) = content.strip_prefix(&[0xfe, 0xff]) {
        encoding_rs::UTF_16BE
            .decode_without_bom_handling_and_without_replacement(value)
            .map(|value| value.into_owned())
    } else if let Ok(value) = std::str::from_utf8(content) {
        Some(value.to_owned())
    } else {
        encoding_rs::GBK
            .decode_without_bom_handling_and_without_replacement(content)
            .map(|value| value.into_owned())
    }
    .ok_or(AttachmentError::InvalidText)?;
    let controls = decoded
        .chars()
        .filter(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        .count();
    if decoded.contains('\0') || controls > decoded.chars().count().saturating_div(100).max(2) {
        return Err(AttachmentError::InvalidText);
    }
    Ok(decoded.into_bytes())
}

fn is_text_extension(extension: &str) -> bool {
    matches!(
        extension,
        "txt"
            | "md"
            | "markdown"
            | "csv"
            | "tsv"
            | "json"
            | "xml"
            | "yaml"
            | "yml"
            | "log"
            | "ini"
            | "conf"
            | "cfg"
            | "toml"
            | "properties"
            | "sql"
    )
}

fn is_text_content_type(content_type: &str) -> bool {
    matches!(
        content_type.split(';').next().unwrap_or_default().trim(),
        "text/plain"
            | "text/markdown"
            | "text/csv"
            | "text/tab-separated-values"
            | "text/xml"
            | "text/yaml"
            | "application/json"
            | "application/xml"
            | "application/yaml"
            | "application/x-yaml"
    )
}

pub fn archive_entries(content: &[u8]) -> Result<Vec<ArchiveEntry>, AttachmentError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(content)).map_err(|_| AttachmentError::InvalidArchive)?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(AttachmentError::TooManyArchiveEntries);
    }
    let mut total = 0_u64;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| AttachmentError::InvalidArchive)?;
        let path = safe_archive_path(file.name())?;
        reject_symlink(file.unix_mode())?;
        total = total.saturating_add(file.size());
        validate_expansion(file.size(), file.compressed_size(), total)?;
        let directory = file.is_dir();
        let name = Path::new(&path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(&path)
            .to_string();
        let (kind, content_type) = if directory {
            (PreviewKind::Unsupported, "inode/directory".to_string())
        } else {
            detect(&name, "application/octet-stream", &[])
        };
        entries.push(ArchiveEntry {
            id: index.to_string(),
            path,
            name,
            size: file.size(),
            directory,
            encrypted: false,
            kind,
            content_type,
        });
    }
    Ok(entries)
}

pub fn read_archive_entry(
    content: &[u8],
    id: &str,
) -> Result<(ArchiveEntry, Vec<u8>), AttachmentError> {
    let index = id
        .parse::<usize>()
        .map_err(|_| AttachmentError::ArchiveEntryMissing)?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(content)).map_err(|_| AttachmentError::InvalidArchive)?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(AttachmentError::TooManyArchiveEntries);
    }
    let file = archive
        .by_index(index)
        .map_err(|_| AttachmentError::ArchiveEntryMissing)?;
    let path = safe_archive_path(file.name())?;
    reject_symlink(file.unix_mode())?;
    validate_expansion(file.size(), file.compressed_size(), file.size())?;
    if file.is_dir() {
        return Err(AttachmentError::ArchiveEntryRejected);
    }
    let name = Path::new(&path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&path)
        .to_string();
    let mut bytes = Vec::with_capacity(file.size().min(MAX_ARCHIVE_ENTRY_BYTES) as usize);
    file.take(MAX_ARCHIVE_ENTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AttachmentError::ArchiveRead)?;
    if bytes.len() as u64 > MAX_ARCHIVE_ENTRY_BYTES {
        return Err(AttachmentError::ArchiveExpandedTooLarge);
    }
    let (kind, content_type) = detect(&name, "application/octet-stream", &bytes);
    let bytes = if kind == PreviewKind::Text {
        normalize_text(&bytes)?
    } else {
        bytes
    };
    Ok((
        ArchiveEntry {
            id: id.to_string(),
            path,
            name,
            size: bytes.len() as u64,
            directory: false,
            encrypted: false,
            kind,
            content_type,
        },
        bytes,
    ))
}

fn validate_expansion(size: u64, compressed: u64, total: u64) -> Result<(), AttachmentError> {
    if size > MAX_ARCHIVE_ENTRY_BYTES || total > MAX_ARCHIVE_TOTAL_BYTES {
        return Err(AttachmentError::ArchiveExpandedTooLarge);
    }
    if size > 1024 * 1024 && size > compressed.max(1).saturating_mul(MAX_ARCHIVE_RATIO) {
        return Err(AttachmentError::ArchiveExpandedTooLarge);
    }
    Ok(())
}

fn safe_archive_path(value: &str) -> Result<String, AttachmentError> {
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    if normalized.starts_with('/')
        || normalized.contains('\0')
        || normalized
            .split('/')
            .next()
            .is_some_and(|segment| segment.contains(':'))
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(AttachmentError::UnsafeArchivePath);
    }
    Ok(normalized.trim_end_matches('/').to_string())
}

fn reject_symlink(mode: Option<u32>) -> Result<(), AttachmentError> {
    if mode.is_some_and(|value| value & 0o170000 == 0o120000) {
        Err(AttachmentError::UnsafeArchivePath)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut output);
            for (name, content) in files {
                writer
                    .start_file(*name, zip::write::FileOptions::default())
                    .unwrap();
                writer.write_all(content).unwrap();
            }
            writer.finish().unwrap();
        }
        output.into_inner()
    }

    #[test]
    fn detects_supported_formats_from_bytes() {
        assert_eq!(detect("wrong.bin", "", b"%PDF-1.7").0, PreviewKind::Pdf);
        assert_eq!(
            detect("photo.bin", "", b"\x89PNG\r\n\x1a\nrest").0,
            PreviewKind::Image
        );
        assert_eq!(
            detect("script.svg", "image/svg+xml", b"<svg/>").0,
            PreviewKind::Unsupported
        );
        assert_eq!(
            detect("fake.pdf", "application/pdf", b"<script>bad</script>").0,
            PreviewKind::Unsupported
        );
        assert_eq!(
            detect("fake.mp4", "video/mp4", b"<script>bad</script>").0,
            PreviewKind::Unsupported
        );
    }

    #[test]
    fn validates_raster_dimensions_before_preview() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&1_u32.to_be_bytes());
        png.extend_from_slice(&1_u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert_eq!(
            inspect("pixel.png", "image/png", &png).unwrap().kind,
            PreviewKind::Image
        );

        png[16..20].copy_from_slice(&(MAX_IMAGE_SIDE as u32 + 1).to_be_bytes());
        assert_eq!(
            inspect("huge.png", "image/png", &png),
            Err(AttachmentError::ImageDimensionsTooLarge)
        );
    }

    #[test]
    fn validates_and_normalizes_common_text_encodings() {
        assert_eq!(
            inspect("readme.md", "text/markdown", "# 标题".as_bytes())
                .unwrap()
                .kind,
            PreviewKind::Text
        );
        let utf16 = [0xff, 0xfe, b'h', 0, b'i', 0];
        assert_eq!(normalize_text(&utf16).unwrap(), b"hi");
        let gbk = encoding_rs::GBK.encode("中文").0.into_owned();
        assert_eq!(normalize_text(&gbk).unwrap(), "中文".as_bytes());
        assert_eq!(
            inspect("fake.txt", "text/plain", b"hello\0binary"),
            Ok(PreviewDescriptor {
                kind: PreviewKind::Unsupported,
                filename: "fake.txt".into(),
                content_type: "application/octet-stream".into(),
                size: 12,
                archive_entries: vec![],
                reason: Some("当前附件类型暂不支持应用内查看".into()),
            })
        );
    }

    #[test]
    fn lists_and_reads_safe_archive_entries() {
        let utf16 = [0xff, 0xfe, b'h', 0, b'i', 0];
        let content = zip(&[("folder/report.pdf", b"%PDF-1.4"), ("note.txt", &utf16)]);
        let descriptor = inspect("files.zip", "application/zip", &content).unwrap();
        assert_eq!(descriptor.kind, PreviewKind::Archive);
        assert_eq!(descriptor.archive_entries.len(), 2);
        let (entry, bytes) = read_archive_entry(&content, "0").unwrap();
        assert_eq!(entry.kind, PreviewKind::Pdf);
        assert_eq!(bytes, b"%PDF-1.4");
        let (entry, bytes) = read_archive_entry(&content, "1").unwrap();
        assert_eq!(entry.kind, PreviewKind::Text);
        assert_eq!(entry.content_type, "text/plain; charset=utf-8");
        assert_eq!(bytes, b"hi");
    }

    #[test]
    fn rejects_parent_paths() {
        let content = zip(&[("../secret.txt", b"secret")]);
        assert_eq!(
            inspect("bad.zip", "application/zip", &content),
            Err(AttachmentError::UnsafeArchivePath)
        );
    }
}
