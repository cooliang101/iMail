import type { MessageStats } from '../../app-model';
import type { Account, Message } from '../../types';

export type MessageChange = { before?: Message; after?: Message };

export function reconcileMessageCache(current: Message[], incoming: Message[]) {
  const currentById = new Map(current.map((message) => [message.id, message]));
  const reconciled = incoming.map((message) => {
    const cached = currentById.get(message.id);
    if (!cached) return message;
    const merged = cached.text !== undefined ? { ...message, text: cached.text, html: cached.html } : message;
    return JSON.stringify(cached) === JSON.stringify(merged) ? cached : merged;
  });
  return current.length === reconciled.length && current.every((message, index) => message === reconciled[index]) ? current : reconciled;
}

export function messageMatchesQuery(message: Message, query: string, accounts: Account[], now = new Date()) {
  const params = new URLSearchParams(query);
  const account = accounts.find((item) => item.id === message.accountId);
  if (params.get('accountId') && params.get('accountId') !== message.accountId) return false;
  if (params.get('group') && params.get('group') !== account?.group) return false;
  if (params.get('mailboxRole') && params.get('mailboxRole') !== message.mailboxRole) return false;
  if (params.get('mailbox') && params.get('mailbox') !== message.mailbox) return false;
  const mailboxName = params.get('mailboxName')?.toLocaleLowerCase();
  if (mailboxName && !account?.mailboxes.some((folder) => folder.name.toLocaleLowerCase() === mailboxName && folder.path === message.mailbox)) return false;
  if (params.get('unread') === 'true' && !message.unread) return false;
  if (params.get('flagged') === 'true' && !message.flagged) return false;
  if (params.get('hasAttachments') === 'true' && !message.hasAttachments) return false;
  if (params.get('label') && !message.labels.includes(params.get('label')!)) return false;
  const snoozed = Boolean(message.snoozedUntil && message.snoozedUntil > now.toISOString());
  if (params.get('snoozed') === 'true' ? !snoozed : message.mailboxRole === 'inbox' && snoozed) return false;
  const search = params.get('q')?.trim().toLocaleLowerCase();
  if (search) {
    const searchable = [message.subject, message.preview, message.from.name, message.from.address, ...message.to.flatMap((recipient) => [recipient.name, recipient.address])].join('\n').toLocaleLowerCase();
    if (!searchable.includes(search)) return false;
  }
  return true;
}

export function applyMessageChanges(current: Message[], changes: MessageChange[], query: string, accounts: Account[]) {
  const next = new Map(current.map((message) => [message.id, message]));
  const cachedById = new Map(current.map((message) => [message.id, message]));
  for (const change of changes) {
    if (change.before) next.delete(change.before.id);
    if (!change.after || !messageMatchesQuery(change.after, query, accounts)) continue;
    const cached = cachedById.get(change.after.id);
    next.set(change.after.id, cached?.text !== undefined ? { ...change.after, text: cached.text, html: cached.html } : change.after);
  }
  const result = [...next.values()].sort((left, right) => right.date.localeCompare(left.date) || right.id.localeCompare(left.id));
  return current.length === result.length && current.every((message, index) => message === result[index]) ? current : result;
}

function inboxContribution(message: Message | undefined, now: Date) {
  const active = Boolean(message && message.mailboxRole === 'inbox' && (!message.snoozedUntil || message.snoozedUntil <= now.toISOString()));
  return { total: active ? 1 : 0, unread: active && message!.unread ? 1 : 0 };
}

export function applyMessageStatsChanges(current: MessageStats, changes: MessageChange[], accounts: Account[], now = new Date()) {
  let total = current.total;
  let unread = current.unread;
  const byAccount = new Map(current.byAccount.map((item) => [item.accountId, { ...item }]));
  const byGroup = new Map(current.byGroup.map((item) => [item.group, { ...item }]));
  for (const change of changes) {
    const before = inboxContribution(change.before, now);
    const after = inboxContribution(change.after, now);
    const totalDelta = after.total - before.total;
    const unreadDelta = after.unread - before.unread;
    if (totalDelta === 0 && unreadDelta === 0) continue;
    const accountId = change.after?.accountId ?? change.before?.accountId;
    if (!accountId) continue;
    const group = accounts.find((account) => account.id === accountId)?.group;
    total = Math.max(0, total + totalDelta); unread = Math.max(0, unread + unreadDelta);
    const accountStats = byAccount.get(accountId) ?? { accountId, total: 0, unread: 0 };
    byAccount.set(accountId, { ...accountStats, total: Math.max(0, accountStats.total + totalDelta), unread: Math.max(0, accountStats.unread + unreadDelta) });
    if (group) {
      const groupStats = byGroup.get(group) ?? { group, total: 0, unread: 0 };
      byGroup.set(group, { ...groupStats, total: Math.max(0, groupStats.total + totalDelta), unread: Math.max(0, groupStats.unread + unreadDelta) });
    }
  }
  return { total, unread, byAccount: [...byAccount.values()], byGroup: [...byGroup.values()] };
}

export function messageTotalDelta(changes: MessageChange[], query: string, accounts: Account[], now = new Date()) {
  return changes.reduce((delta, change) => delta
    + Number(Boolean(change.after && messageMatchesQuery(change.after, query, accounts, now)))
    - Number(Boolean(change.before && messageMatchesQuery(change.before, query, accounts, now))), 0);
}
