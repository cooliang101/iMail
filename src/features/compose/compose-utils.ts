import type { DraftAttachment } from '../../types';

export function subjectWithPrefix(subject: string, prefix: 'Re' | 'Fwd') { return new RegExp(`^${prefix}:`, 'i').test(subject) ? subject : `${prefix}: ${subject}`; }

export function textToHtml(value: string) {
  const escapeHtml = (text: string) => text.replace(/[&<>"']/g, (character) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#039;' })[character] ?? character);
  return value.split('\n').map((line) => `<p>${line ? escapeHtml(line) : '<br>'}</p>`).join('');
}

export function formatAttachmentSize(size: number) { return size < 1024 * 1024 ? `${Math.max(1, Math.round(size / 1024))} KB` : `${(size / 1024 / 1024).toFixed(1)} MB`; }

export function fileAsAttachment(file: File) {
  return new Promise<DraftAttachment>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve({ id: crypto.randomUUID(), filename: file.name, contentType: file.type || 'application/octet-stream', size: file.size, data: String(reader.result).split(',')[1] ?? '' });
    reader.onerror = () => reject(new Error(`无法读取 ${file.name}`));
    reader.readAsDataURL(file);
  });
}
