import type { CachedMessage, MailAccount } from '../types.js';
import { integrationMessageSummary } from '../domain/message-views.js';

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

export function gatewayMessageSummary(message: CachedMessage, accountEmail: string) {
  return integrationMessageSummary(message, accountEmail);
}

export function gatewayMessageDetail(message: CachedMessage, accountEmail: string) {
  return { ...integrationMessageSummary(message, accountEmail), text: message.text, ...(message.html ? { html: message.html } : {}) };
}
