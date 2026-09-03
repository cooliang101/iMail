import type { PlatformRuntime } from '../../platform/types';
import { absoluteServiceUrl } from '../../services';

export function rawMessageDownloadPath(messageId: string) {
  return `/api/messages/${encodeURIComponent(messageId)}/source/download`;
}

export function rawMessageFilename(subject: string) {
  const cleaned = subject
    .trim()
    .replace(/[<>:"/\\|?*\p{Cc}]/gu, '_')
    .slice(0, 96)
    .replace(/[. ]+$/g, '');
  const stem = !cleaned
    ? '原始邮件'
    : /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:[. ]|$)/i.test(cleaned) ? `_${cleaned}` : cleaned;
  return `${stem}.eml`;
}

export async function fetchRawMessageBlob(url: string, fetcher: typeof fetch = fetch) {
  const response = await fetcher(url, { credentials: 'include' });
  if (!response.ok) {
    const body = await response.json().catch(() => ({})) as { error?: unknown };
    throw new Error(typeof body.error === 'string' && body.error.trim() ? body.error : `原始邮件下载失败（${response.status}）`);
  }
  if (response.headers.get('content-type')?.split(';', 1)[0].trim().toLocaleLowerCase() !== 'message/rfc822') throw new Error('服务器返回的原始邮件格式不正确');
  return response.blob();
}

export async function downloadRawMessage(platform: PlatformRuntime, message: { id: string; subject: string }) {
  const path = rawMessageDownloadPath(message.id);
  const filename = rawMessageFilename(message.subject);
  if (platform.kind === 'tauri') { await platform.saveDownload({ url: path, filename }); return; }
  const blobUrl = URL.createObjectURL(await fetchRawMessageBlob(absoluteServiceUrl(path)));
  try { await platform.saveDownload({ url: blobUrl, filename }); }
  finally { window.setTimeout(() => URL.revokeObjectURL(blobUrl), 0); }
}
