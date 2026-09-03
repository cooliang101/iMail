import { absoluteServiceUrl, api, desktopReadBinary } from '../../services';
import type { MessageAttachment } from '../../types';
export { formatFileSize as formatAttachmentSize } from '../../components/file-utils';

export type PreviewKind = 'image' | 'pdf' | 'video' | 'archive' | 'text' | 'unsupported';

export type ArchiveEntry = {
  id: string;
  path: string;
  name: string;
  size: number;
  directory: boolean;
  encrypted: boolean;
  kind: PreviewKind;
  contentType: string;
};

export type PreviewDescriptor = {
  kind: PreviewKind;
  filename: string;
  contentType: string;
  size: number;
  archiveEntries: ArchiveEntry[];
  reason?: string;
};

export type PreviewSession = {
  previewId: string;
  descriptor: PreviewDescriptor;
  expiresInSeconds: number;
};

const previewExtensions = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'pdf', 'mp4', 'webm', 'ogv', 'ogg', 'zip', 'txt', 'md', 'markdown', 'csv', 'tsv', 'json', 'xml', 'yaml', 'yml', 'log', 'ini', 'conf', 'cfg', 'toml', 'properties', 'sql']);
const previewMimePrefixes = ['image/jpeg', 'image/png', 'image/gif', 'image/webp', 'image/bmp', 'application/pdf', 'application/zip', 'video/mp4', 'video/webm', 'video/ogg', 'text/plain', 'text/markdown', 'text/csv', 'text/tab-separated-values', 'application/json', 'application/xml', 'text/xml', 'application/yaml', 'text/yaml', 'application/x-yaml'];

export function canPreviewAttachment(attachment: Pick<MessageAttachment, 'filename' | 'contentType'>) {
  const contentType = attachment.contentType.toLowerCase().split(';')[0].trim();
  const extension = attachment.filename.toLowerCase().split('.').pop() ?? '';
  return previewMimePrefixes.includes(contentType) || previewExtensions.has(extension);
}

export function createPreview(messageId: string, index: number) {
  return api<PreviewSession>(`/api/messages/${encodeURIComponent(messageId)}/attachments/${index}/preview`, { method: 'POST' });
}

export function deletePreview(previewId: string) {
  return api<void>(`/api/attachment-previews/${encodeURIComponent(previewId)}`, { method: 'DELETE' });
}

export function previewContentPath(previewId: string, entryId?: string) {
  const root = `/api/attachment-previews/${encodeURIComponent(previewId)}`;
  return entryId === undefined
    ? `${root}/content`
    : `${root}/archive/entries/${encodeURIComponent(entryId)}`;
}

export async function resolvePreviewSource(path: string, contentType: string, tauri: boolean) {
  if (!tauri) return { url: absoluteServiceUrl(path), revoke: () => undefined };
  const bytes = await desktopReadBinary(path);
  const copy = new Uint8Array(bytes.byteLength);
  copy.set(bytes);
  const url = URL.createObjectURL(new Blob([copy.buffer], { type: contentType }));
  return { url, revoke: () => URL.revokeObjectURL(url) };
}

export async function resolvePreviewText(path: string, tauri: boolean) {
  if (tauri) {
    const bytes = await desktopReadBinary(path);
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  }
  const response = await fetch(absoluteServiceUrl(path), { credentials: 'include' });
  if (!response.ok) throw new Error(`文本内容读取失败（${response.status}）`);
  return response.text();
}
