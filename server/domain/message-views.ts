import type { CachedMessage } from '../types.js';

function attachments(message: CachedMessage) {
  return message.attachments.map((attachment, index) => ({ ...attachment, index: attachment.index ?? index }));
}

export function clientMessageSummary(message: CachedMessage) {
  const { uid: _uid, messageId: _messageId, text: _text, html: _html, ...summary } = message;
  return {
    ...summary,
    mailboxRole: summary.mailboxRole ?? 'inbox',
    attachments: attachments(message),
    labels: summary.labels ?? [],
    from: { ...summary.from, logo: { url: `/api/contacts/logo?address=${encodeURIComponent(summary.from.address)}` } },
  };
}

export function integrationMessageSummary(message: CachedMessage, accountEmail: string) {
  return {
    id: message.id, accountEmail, folder: message.mailbox, mailboxRole: message.mailboxRole ?? 'inbox',
    from: message.from, to: message.to, subject: message.subject, preview: message.preview, date: message.date,
    unread: message.unread, flagged: message.flagged, hasAttachments: message.hasAttachments,
    attachments: attachments(message), labels: message.labels ?? [],
  };
}
