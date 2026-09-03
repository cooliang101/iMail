import type { DraftAttachment } from '../../types';
import { readFileAsDataUrl } from '../../components/file-utils';

export function subjectWithPrefix(subject: string, prefix: 'Re' | 'Fwd') { return new RegExp(`^${prefix}:`, 'i').test(subject) ? subject : `${prefix}: ${subject}`; }

export function textToHtml(value: string) {
  const escapeHtml = (text: string) => text.replace(/[&<>"']/g, (character) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#039;' })[character] ?? character);
  return value.split('\n').map((line) => `<p>${line ? escapeHtml(line) : '<br>'}</p>`).join('');
}

export { formatFileSize as formatAttachmentSize } from '../../components/file-utils';

export async function fileAsAttachment(file: File): Promise<DraftAttachment> {
  const dataUrl = await readFileAsDataUrl(file);
  return { id: crypto.randomUUID(), filename: file.name, contentType: file.type || 'application/octet-stream', size: file.size, data: dataUrl.split(',')[1] ?? '' };
}
