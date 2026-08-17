import type { DesktopHttpInvoker, DesktopHttpResponse } from '../desktop/http';
import type { MailService } from './contracts';

export type EmbeddedDomainCall =
  | { operation: 'systemInfo' }
  | { operation: 'authStatus' }
  | { operation: 'authRegister'; input: Record<string, unknown> }
  | { operation: 'authLogin'; input: Record<string, unknown> }
  | { operation: 'authLogout' }
  | { operation: 'accountsList' }
  | { operation: 'accountCreate'; input: Record<string, unknown> }
  | { operation: 'accountUpdate'; accountId: string; input: Record<string, unknown> }
  | { operation: 'accountDelete'; accountId: string }
  | { operation: 'accountCredentialUpdate'; accountId: string; input: Record<string, unknown> }
  | { operation: 'accountProxyUpdate'; accountId: string; input: Record<string, unknown> }
  | { operation: 'accountConnectionTest'; accountId: string }
  | { operation: 'oauthStart'; input: Record<string, unknown> }
  | { operation: 'oauthReconnect'; accountId: string }
  | { operation: 'oauthStatus'; input: Record<string, unknown> }
  | { operation: 'messageStats' }
  | { operation: 'messagesList'; query: Record<string, string> }
  | { operation: 'messageDetail'; messageId: string }
  | { operation: 'labelsList' }
  | { operation: 'contactsList' }
  | { operation: 'notificationsList' }
  | { operation: 'syncAll' }
  | { operation: 'syncAccount'; accountId: string }
  | { operation: 'syncAccountMailbox'; accountId: string; mailbox: string }
  | { operation: 'syncMailboxRole'; role: string }
  | { operation: 'messageUpdate'; messageId: string; unread?: boolean; flagged?: boolean; labels?: string[]; snoozedUntil?: unknown }
  | { operation: 'messageMove'; messageId: string; destination: string }
  | { operation: 'attachmentPreviewCreate'; messageId: string; index: number }
  | { operation: 'attachmentPreviewDelete'; previewId: string }
  | { operation: 'messageSend'; input: Record<string, unknown> }
  | { operation: 'draftsList' }
  | { operation: 'draftCreate'; draftId?: string; input: Record<string, unknown> }
  | { operation: 'draftUpdate'; draftId: string; input: Record<string, unknown> }
  | { operation: 'draftDelete'; draftId: string }
  | { operation: 'preferencesGet' }
  | { operation: 'preferencesUpdate'; input: Record<string, unknown> }
  | { operation: 'developerTokensList' }
  | { operation: 'developerTokenCreate'; input: Record<string, unknown> }
  | { operation: 'developerTokenDelete'; tokenId: string }
  | { operation: 'externalAccessGet' }
  | { operation: 'externalAccessUpdate'; input: Record<string, unknown> }
  | { operation: 'authorizationExportPrepare'; input: Record<string, unknown> }
  | { operation: 'userDataClear'; input: Record<string, unknown> };

function jsonRecord(body: BodyInit | null | undefined) {
  if (typeof body !== 'string') return null;
  try {
    const value = JSON.parse(body) as unknown;
    return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null;
  } catch {
    return null;
  }
}

export function embeddedDomainCall(path: string, options: RequestInit = {}): EmbeddedDomainCall | null {
  const method = (options.method ?? 'GET').toUpperCase();
  const url = new URL(path, 'http://embedded.local');
  if (url.origin !== 'http://embedded.local') return null;
  if (method === 'GET' && options.body === undefined) {
    const reads: Record<string, EmbeddedDomainCall> = {
      '/api/auth/status': { operation: 'authStatus' },
      '/api/system/info': { operation: 'systemInfo' },
      '/api/accounts': { operation: 'accountsList' },
      '/api/message-stats': { operation: 'messageStats' },
      '/api/labels': { operation: 'labelsList' },
      '/api/contacts': { operation: 'contactsList' },
      '/api/notifications': { operation: 'notificationsList' },
      '/api/drafts': { operation: 'draftsList' },
      '/api/preferences': { operation: 'preferencesGet' },
      '/api/developer-tokens': { operation: 'developerTokensList' },
      '/api/external-access': { operation: 'externalAccessGet' },
    };
    if (reads[url.pathname]) return reads[url.pathname];
    if (url.pathname === '/api/messages') return { operation: 'messagesList', query: Object.fromEntries(url.searchParams) };
  }
  const detail = url.pathname.match(/^\/api\/messages\/([^/]+)$/);
  if (method === 'GET' && options.body === undefined && detail) return { operation: 'messageDetail', messageId: decodeURIComponent(detail[1]) };
  if (method === 'POST' && options.body === undefined && url.pathname === '/api/sync') return { operation: 'syncAll' };
  const accountSync = url.pathname.match(/^\/api\/accounts\/([^/]+)\/sync$/);
  if (method === 'POST' && options.body === undefined && accountSync) return { operation: 'syncAccount', accountId: decodeURIComponent(accountSync[1]) };
  const accountMailbox = url.pathname.match(/^\/api\/accounts\/([^/]+)\/mailboxes\/sync$/);
  const body = jsonRecord(options.body);
  if (method === 'POST' && accountMailbox && typeof body?.mailbox === 'string') {
    return { operation: 'syncAccountMailbox', accountId: decodeURIComponent(accountMailbox[1]), mailbox: body.mailbox };
  }
  const mailboxRole = url.pathname.match(/^\/api\/mailboxes\/([^/]+)\/sync$/);
  if (method === 'POST' && options.body === undefined && mailboxRole) return { operation: 'syncMailboxRole', role: decodeURIComponent(mailboxRole[1]) };
  if (method === 'PATCH' && detail && body) {
    return {
      operation: 'messageUpdate', messageId: decodeURIComponent(detail[1]),
      ...(typeof body.unread === 'boolean' ? { unread: body.unread } : {}),
      ...(typeof body.flagged === 'boolean' ? { flagged: body.flagged } : {}),
      ...(Array.isArray(body.labels) ? { labels: body.labels.filter((label): label is string => typeof label === 'string') } : {}),
      ...('snoozedUntil' in body ? { snoozedUntil: body.snoozedUntil } : {}),
    };
  }
  const move = url.pathname.match(/^\/api\/messages\/([^/]+)\/move$/);
  if (method === 'POST' && move && typeof body?.destination === 'string') return { operation: 'messageMove', messageId: decodeURIComponent(move[1]), destination: body.destination };
  const attachmentPreview = url.pathname.match(/^\/api\/messages\/([^/]+)\/attachments\/(\d+)\/preview$/);
  if (method === 'POST' && options.body === undefined && attachmentPreview) return { operation: 'attachmentPreviewCreate', messageId: decodeURIComponent(attachmentPreview[1]), index: Number(attachmentPreview[2]) };
  const preview = url.pathname.match(/^\/api\/attachment-previews\/([^/]+)$/);
  if (method === 'DELETE' && options.body === undefined && preview) return { operation: 'attachmentPreviewDelete', previewId: decodeURIComponent(preview[1]) };
  if (method === 'POST' && url.pathname === '/api/auth/register' && body) return { operation: 'authRegister', input: body };
  if (method === 'POST' && url.pathname === '/api/auth/login' && body) return { operation: 'authLogin', input: body };
  if (method === 'POST' && url.pathname === '/api/auth/logout' && options.body === undefined) return { operation: 'authLogout' };
  if (method === 'POST' && url.pathname === '/api/accounts' && body) return { operation: 'accountCreate', input: body };
  const account = url.pathname.match(/^\/api\/accounts\/([^/]+)$/);
  if (account && method === 'PATCH' && body) return { operation: 'accountUpdate', accountId: decodeURIComponent(account[1]), input: body };
  if (account && method === 'DELETE' && options.body === undefined) return { operation: 'accountDelete', accountId: decodeURIComponent(account[1]) };
  const accountCredential = url.pathname.match(/^\/api\/accounts\/([^/]+)\/credential$/);
  if (accountCredential && method === 'PUT' && body) return { operation: 'accountCredentialUpdate', accountId: decodeURIComponent(accountCredential[1]), input: body };
  const accountProxy = url.pathname.match(/^\/api\/accounts\/([^/]+)\/proxy$/);
  if (accountProxy && method === 'PUT' && body) return { operation: 'accountProxyUpdate', accountId: decodeURIComponent(accountProxy[1]), input: body };
  const accountTest = url.pathname.match(/^\/api\/accounts\/([^/]+)\/connection-test$/);
  if (accountTest && method === 'POST' && options.body === undefined) return { operation: 'accountConnectionTest', accountId: decodeURIComponent(accountTest[1]) };
  const oauthReconnect = url.pathname.match(/^\/api\/accounts\/([^/]+)\/oauth\/reconnect$/);
  if (oauthReconnect && method === 'POST' && options.body === undefined) return { operation: 'oauthReconnect', accountId: decodeURIComponent(oauthReconnect[1]) };
  if (method === 'POST' && url.pathname === '/api/oauth/start' && body) return { operation: 'oauthStart', input: body };
  if (method === 'POST' && url.pathname === '/api/oauth/status' && body) return { operation: 'oauthStatus', input: body };
  if (method === 'POST' && url.pathname === '/api/send' && body) return { operation: 'messageSend', input: body };
  if (method === 'POST' && url.pathname === '/api/drafts' && body) {
    const draftId = new Headers(options.headers).get('X-Draft-Id') ?? undefined;
    return { operation: 'draftCreate', ...(draftId ? { draftId } : {}), input: body };
  }
  const draft = url.pathname.match(/^\/api\/drafts\/([^/]+)$/);
  if (draft && method === 'PUT' && body) return { operation: 'draftUpdate', draftId: decodeURIComponent(draft[1]), input: body };
  if (draft && method === 'DELETE' && options.body === undefined) return { operation: 'draftDelete', draftId: decodeURIComponent(draft[1]) };
  if (method === 'PATCH' && url.pathname === '/api/preferences' && body) return { operation: 'preferencesUpdate', input: body };
  if (method === 'POST' && url.pathname === '/api/developer-tokens' && body) return { operation: 'developerTokenCreate', input: body };
  const token = url.pathname.match(/^\/api\/developer-tokens\/([^/]+)$/);
  if (token && method === 'DELETE' && options.body === undefined) return { operation: 'developerTokenDelete', tokenId: decodeURIComponent(token[1]) };
  if (method === 'PATCH' && url.pathname === '/api/external-access' && body) return { operation: 'externalAccessUpdate', input: body };
  if (method === 'POST' && url.pathname === '/api/security/mail-authorization-exports' && body) return { operation: 'authorizationExportPrepare', input: body };
  if (method === 'POST' && url.pathname === '/api/security/clear-user-data' && body) return { operation: 'userDataClear', input: body };
  return null;
}

export class TauriMailService implements MailService {
  readonly kind = 'tauri-embedded' as const;
  constructor(private readonly invoker: DesktopHttpInvoker) {}

  request(path: string, options: RequestInit = {}) {
    if (options.body !== undefined && typeof options.body !== 'string') {
      throw new Error('Tauri 直连只接受 JSON 请求体');
    }
    const call = embeddedDomainCall(path, options);
    if (call) return this.invoker<DesktopHttpResponse>('desktop_mail_service_call', { call });
    throw new Error(`Tauri 直连尚未映射该领域操作：${options.method ?? 'GET'} ${path.split('?')[0]}`);
  }
}
