import type { StoreData } from '../types.js';

export function buildNotifications(data: StoreData, limit = 30, now = new Date().toISOString()) {
  const connection = data.accounts.filter((item) => item.status === 'error').map((account) => ({
    id: `account-${account.id}`, kind: 'error' as const, title: `${account.displayName} 连接异常`,
    detail: account.lastError || account.email, date: account.lastSyncAt || account.createdAt, accountId: account.id,
  }));
  const returned = data.messages.filter((item) => item.snoozedUntil && item.snoozedUntil <= now).map((message) => ({
    id: `snooze-${message.id}`, kind: 'snooze' as const, title: message.subject, detail: '稍后处理的邮件已返回收件箱',
    date: message.snoozedUntil!, messageId: message.id, accountId: message.accountId,
  }));
  const unread = data.messages.filter((item) => (item.mailboxRole ?? 'inbox') === 'inbox' && item.unread && (!item.snoozedUntil || item.snoozedUntil <= now)).map((message) => ({
    id: `unread-${message.id}`, kind: 'unread' as const, title: message.subject, detail: message.from.name || message.from.address,
    date: message.date, messageId: message.id, accountId: message.accountId,
  }));
  return [...connection, ...returned, ...unread].sort((a, b) => b.date.localeCompare(a.date)).slice(0, limit);
}
