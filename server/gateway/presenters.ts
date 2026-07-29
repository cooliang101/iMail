import type { CachedMessage, MailAccount } from '../types.js';

export function gatewayMailbox(account: MailAccount) {
  return {
    email: account.email,
    provider: account.provider,
    displayName: account.displayName,
    group: account.group,
    status: account.status,
    lastSyncAt: account.lastSyncAt,
  };
}

function messageBase(message: CachedMessage, accountEmail: string) {
  return {
    id: message.id,
    accountEmail,
    folder: message.mailbox,
    mailboxRole: message.mailboxRole ?? 'inbox',
    from: message.from,
    to: message.to,
    subject: message.subject,
    preview: message.preview,
    date: message.date,
    unread: message.unread,
    flagged: message.flagged,
    hasAttachments: message.hasAttachments,
    attachments: message.attachments.map(({ filename, contentType, size, index }, fallbackIndex) => ({ filename, contentType, size, index: index ?? fallbackIndex })),
    labels: message.labels ?? [],
  };
}

export function gatewayMessageSummary(message: CachedMessage, accountEmail: string) {
  return messageBase(message, accountEmail);
}

export function gatewayMessageDetail(message: CachedMessage, accountEmail: string) {
  return { ...messageBase(message, accountEmail), text: message.text, ...(message.html ? { html: message.html } : {}) };
}
