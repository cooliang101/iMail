import type { CachedMessage, MailboxRole } from '../types.js';

export type SqlValue = string | number | bigint | null | Uint8Array;
export type Row = Record<string, SqlValue>;

export function text(row: Row, key: string) { return String(row[key] ?? ''); }
export function optionalText(row: Row, key: string) { return row[key] === null || row[key] === undefined ? undefined : String(row[key]); }
export function json<T>(row: Row, key: string): T { return JSON.parse(text(row, key)) as T; }
export function integer(row: Row, key: string) { return Number(row[key]); }

export function messageFromRow(row: Row): CachedMessage {
  const message: CachedMessage = {
    id: text(row, 'id'), accountId: text(row, 'account_id'), mailbox: text(row, 'mailbox'), mailboxRole: text(row, 'mailbox_role') as MailboxRole, uid: integer(row, 'uid'),
    messageId: optionalText(row, 'message_id'), from: json(row, 'from_json'), to: json(row, 'to_json'),
    subject: text(row, 'subject'), preview: text(row, 'preview'), text: text(row, 'text_body'), html: optionalText(row, 'html_body'),
    date: text(row, 'received_at'), unread: Boolean(integer(row, 'unread')), flagged: Boolean(integer(row, 'flagged')),
    hasAttachments: Boolean(integer(row, 'has_attachments')), attachments: json<Array<{ filename: string; contentType: string; size: number; index?: number }>>(row, 'attachments_json').map((item, index) => ({ ...item, index: item.index ?? index })),
    labels: json(row, 'labels_json'),
  };
  const snoozedUntil = optionalText(row, 'snoozed_until');
  if (snoozedUntil) message.snoozedUntil = snoozedUntil;
  return message;
}
