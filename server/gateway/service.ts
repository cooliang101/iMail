import type { DeveloperToken, StoreData } from '../types.js';
import type { GatewayMessageQuery } from './contracts.js';
import { GatewayError } from './errors.js';
import { gatewayMailbox, gatewayMessageDetail, gatewayMessageSummary } from './presenters.js';

type Cursor = { date: string; id: string };

function encodeCursor(cursor: Cursor) {
  return Buffer.from(JSON.stringify(cursor)).toString('base64url');
}

function decodeCursor(value?: string): Cursor | undefined {
  if (!value) return undefined;
  try {
    const parsed = JSON.parse(Buffer.from(value, 'base64url').toString('utf8')) as Partial<Cursor>;
    if (typeof parsed.date !== 'string' || typeof parsed.id !== 'string') throw new Error();
    return { date: parsed.date, id: parsed.id };
  } catch {
    throw new GatewayError(400, 'INVALID_CURSOR', '分页游标无效或已损坏');
  }
}

function authorizedAccount(data: StoreData, token: DeveloperToken, email: string) {
  const account = data.accounts.find((item) => item.email.toLowerCase() === email.toLowerCase());
  if (!account || !token.accountIds.includes(account.id)) throw new GatewayError(404, 'MAILBOX_NOT_AVAILABLE', '邮箱不存在或未授权');
  return account;
}

export function listGatewayMailboxes(data: StoreData, token: DeveloperToken) {
  return data.accounts.filter((account) => token.accountIds.includes(account.id)).map(gatewayMailbox);
}

export function listGatewayMessages(data: StoreData, token: DeveloperToken, query: GatewayMessageQuery) {
  const selected = query.mailbox ? authorizedAccount(data, token, query.mailbox) : undefined;
  const permittedIds = new Set(selected ? [selected.id] : token.accountIds);
  const cursor = decodeCursor(query.cursor);
  const normalizedQuery = query.q?.toLocaleLowerCase();
  const filtered = data.messages.filter((message) => {
    if (!permittedIds.has(message.accountId)) return false;
    if (query.mailboxRole && (message.mailboxRole ?? 'inbox') !== query.mailboxRole) return false;
    if (query.unread !== undefined && message.unread !== query.unread) return false;
    if (query.since && Date.parse(message.date) < Date.parse(query.since)) return false;
    if (query.before && Date.parse(message.date) >= Date.parse(query.before)) return false;
    if (normalizedQuery && !`${message.subject}\n${message.preview}\n${message.from.name}\n${message.from.address}`.toLocaleLowerCase().includes(normalizedQuery)) return false;
    if (cursor && !(message.date < cursor.date || (message.date === cursor.date && message.id < cursor.id))) return false;
    return true;
  }).sort((a, b) => b.date.localeCompare(a.date) || b.id.localeCompare(a.id));
  const page = filtered.slice(0, query.limit + 1);
  const hasMore = page.length > query.limit;
  const selectedMessages = page.slice(0, query.limit);
  const accountsById = new Map(data.accounts.map((account) => [account.id, account.email]));
  const messages = selectedMessages.map((message) => gatewayMessageSummary(message, accountsById.get(message.accountId)!));
  const last = selectedMessages.at(-1);
  return { messages, page: { limit: query.limit, count: messages.length, hasMore, nextCursor: hasMore && last ? encodeCursor({ date: last.date, id: last.id }) : null } };
}

export function getGatewayMessage(data: StoreData, token: DeveloperToken, messageId: string) {
  const message = data.messages.find((item) => item.id === messageId && token.accountIds.includes(item.accountId));
  if (!message) throw new GatewayError(404, 'MESSAGE_NOT_FOUND', '邮件不存在或无权访问');
  const account = data.accounts.find((item) => item.id === message.accountId);
  if (!account) throw new GatewayError(404, 'MESSAGE_NOT_FOUND', '邮件不存在或无权访问');
  return { source: message, response: gatewayMessageDetail(message, account.email) };
}

export function assertGatewayAttachment(data: StoreData, token: DeveloperToken, messageId: string, index: number) {
  const message = getGatewayMessage(data, token, messageId);
  if (!message.source.attachments[index]) throw new GatewayError(404, 'ATTACHMENT_NOT_FOUND', '附件不存在或无权访问');
}

export function getGatewaySendingAccount(data: StoreData, token: DeveloperToken, mailbox: string) {
  return authorizedAccount(data, token, mailbox);
}
