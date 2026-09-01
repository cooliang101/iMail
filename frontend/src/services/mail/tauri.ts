import { invokeDesktopWithAbort, type DesktopHttpInvoker, type DesktopHttpResponse } from '../desktop/http';
import type { MailService } from './contracts';

export type EmbeddedDomainCall =
  | { operation: 'systemInfo' }
  | { operation: 'providers' }
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
  | { operation: 'appleHmeStatus'; accountId: string }
  | { operation: 'appleHmeStartLogin'; accountId: string; input: Record<string, unknown> }
  | { operation: 'appleHmeSubmitTwoFactor'; accountId: string; input: Record<string, unknown> }
  | { operation: 'appleHmeList'; accountId: string }
  | { operation: 'appleHmeSync'; accountId: string }
  | { operation: 'appleHmeCreate'; accountId: string; input: Record<string, unknown> }
  | { operation: 'appleHmeDeactivate'; accountId: string; anonymousId: string }
  | { operation: 'appleHmeDelete'; accountId: string; anonymousId: string }
  | { operation: 'appleHmeDisconnect'; accountId: string }
  | { operation: 'oauthStart'; input: Record<string, unknown> }
  | { operation: 'oauthReconnect'; accountId: string }
  | { operation: 'oauthStatus'; input: Record<string, unknown> }
  | { operation: 'messageStats' }
  | { operation: 'messagesList'; query: Record<string, string> }
  | { operation: 'messageDetail'; messageId: string }
  | { operation: 'messageSource'; messageId: string }
  | { operation: 'messageConversation'; messageId: string }
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
  | { operation: 'outboxList' }
  | { operation: 'outboxSchedule'; input: Record<string, unknown> }
  | { operation: 'outboxCancel'; itemId: string }
  | { operation: 'outboxRetry'; itemId: string }
  | { operation: 'outboxResolve'; itemId: string; input: Record<string, unknown> }
  | { operation: 'mailWorkItemsList' }
  | { operation: 'mailWorkItemSet'; messageId: string; input: Record<string, unknown> }
  | { operation: 'mailWorkItemComplete'; messageId: string }
  | { operation: 'mailReplyDraftCreate'; messageId: string; input: Record<string, unknown> }
  | { operation: 'mailDraftSchedule'; draftId: string; input: Record<string, unknown> }
  | { operation: 'draftsList' }
  | { operation: 'draftCreate'; draftId?: string; input: Record<string, unknown> }
  | { operation: 'draftUpdate'; draftId: string; input: Record<string, unknown> }
  | { operation: 'draftDelete'; draftId: string }
  | { operation: 'preferencesGet' }
  | { operation: 'smartFoldersList' }
  | { operation: 'mailRulesList' }
  | { operation: 'mailRuleGet'; ruleId: string }
  | { operation: 'mailRuleCreate'; input: Record<string, unknown> }
  | { operation: 'mailRuleUpdate'; ruleId: string; input: Record<string, unknown> }
  | { operation: 'mailRuleSetEnabled'; ruleId: string; input: Record<string, unknown> }
  | { operation: 'mailRuleDelete'; ruleId: string }
  | { operation: 'mailRulePreview'; input: Record<string, unknown> }
  | { operation: 'mailRuleApply'; input: Record<string, unknown> }
  | { operation: 'mailRuleRuns' }
  | { operation: 'mailRuleRetry'; runId: string }
  | { operation: 'smartFolderCreate'; input: Record<string, unknown> }
  | { operation: 'smartFolderUpdate'; folderId: string; input: Record<string, unknown> }
  | { operation: 'smartFolderDelete'; folderId: string }
  | { operation: 'preferencesUpdate'; input: Record<string, unknown> }
  | { operation: 'translationSettingsGet' }
  | { operation: 'translationSettingsUpdate'; input: Record<string, unknown> }
  | { operation: 'translationProfileUpsert'; profileId: string; input: Record<string, unknown> }
  | { operation: 'translationProfileDelete'; profileId: string }
  | { operation: 'translationCredentialUpdate'; profileId: string; input: Record<string, unknown> }
  | { operation: 'translationCredentialClear'; profileId: string }
  | { operation: 'translationConsentAccept'; profileId: string }
  | { operation: 'translationConsentRevoke'; profileId: string }
  | { operation: 'translationPrepare'; messageId: string; input: Record<string, unknown> }
  | { operation: 'translationExecute'; messageId: string; input: Record<string, unknown> }
  | { operation: 'translationComplete'; messageId: string; input: Record<string, unknown> }
  | { operation: 'translationCacheClear' }
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
      '/api/providers': { operation: 'providers' },
      '/api/accounts': { operation: 'accountsList' },
      '/api/message-stats': { operation: 'messageStats' },
      '/api/labels': { operation: 'labelsList' },
      '/api/contacts': { operation: 'contactsList' },
      '/api/notifications': { operation: 'notificationsList' },
      '/api/drafts': { operation: 'draftsList' },
      '/api/outbox': { operation: 'outboxList' },
      '/api/mail-work-items': { operation: 'mailWorkItemsList' },
      '/api/preferences': { operation: 'preferencesGet' },
      '/api/smart-folders': { operation: 'smartFoldersList' },
      '/api/mail-rules': { operation: 'mailRulesList' },
      '/api/mail-rule-runs': { operation: 'mailRuleRuns' },
      '/api/translation-settings': { operation: 'translationSettingsGet' },
      '/api/developer-tokens': { operation: 'developerTokensList' },
      '/api/external-access': { operation: 'externalAccessGet' },
    };
    if (reads[url.pathname]) return reads[url.pathname];
    if (url.pathname === '/api/messages') return { operation: 'messagesList', query: Object.fromEntries(url.searchParams) };
  }
  const source = url.pathname.match(/^\/api\/messages\/([^/]+)\/source$/);
  const conversation = url.pathname.match(/^\/api\/messages\/([^/]+)\/conversation$/);
  if (method === 'GET' && options.body === undefined && conversation) return { operation: 'messageConversation', messageId: decodeURIComponent(conversation[1]) };
  if (method === 'GET' && options.body === undefined && source) return { operation: 'messageSource', messageId: decodeURIComponent(source[1]) };
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
  const appleHme = url.pathname.match(/^\/api\/accounts\/([^/]+)\/apple-hme$/);
  if (appleHme && method === 'GET' && options.body === undefined) return { operation: 'appleHmeStatus', accountId: decodeURIComponent(appleHme[1]) };
  if (appleHme && method === 'DELETE' && options.body === undefined) return { operation: 'appleHmeDisconnect', accountId: decodeURIComponent(appleHme[1]) };
  const appleHmeLogin = url.pathname.match(/^\/api\/accounts\/([^/]+)\/apple-hme\/login$/);
  if (appleHmeLogin && method === 'POST' && body) return { operation: 'appleHmeStartLogin', accountId: decodeURIComponent(appleHmeLogin[1]), input: body };
  const appleHmeTwoFactor = url.pathname.match(/^\/api\/accounts\/([^/]+)\/apple-hme\/two-factor$/);
  if (appleHmeTwoFactor && method === 'POST' && body) return { operation: 'appleHmeSubmitTwoFactor', accountId: decodeURIComponent(appleHmeTwoFactor[1]), input: body };
  const appleHmeAddresses = url.pathname.match(/^\/api\/accounts\/([^/]+)\/apple-hme\/addresses$/);
  if (appleHmeAddresses && method === 'GET' && options.body === undefined) return { operation: 'appleHmeList', accountId: decodeURIComponent(appleHmeAddresses[1]) };
  if (appleHmeAddresses && method === 'POST' && body) return { operation: 'appleHmeCreate', accountId: decodeURIComponent(appleHmeAddresses[1]), input: body };
  const appleHmeSync = url.pathname.match(/^\/api\/accounts\/([^/]+)\/apple-hme\/addresses\/sync$/);
  if (appleHmeSync && method === 'POST' && options.body === undefined) return { operation: 'appleHmeSync', accountId: decodeURIComponent(appleHmeSync[1]) };
  const appleHmeAddress = url.pathname.match(/^\/api\/accounts\/([^/]+)\/apple-hme\/addresses\/([^/]+)$/);
  if (appleHmeAddress && method === 'DELETE' && options.body === undefined) return { operation: 'appleHmeDelete', accountId: decodeURIComponent(appleHmeAddress[1]), anonymousId: decodeURIComponent(appleHmeAddress[2]) };
  const appleHmeDeactivate = url.pathname.match(/^\/api\/accounts\/([^/]+)\/apple-hme\/addresses\/([^/]+)\/deactivate$/);
  if (appleHmeDeactivate && method === 'POST' && options.body === undefined) return { operation: 'appleHmeDeactivate', accountId: decodeURIComponent(appleHmeDeactivate[1]), anonymousId: decodeURIComponent(appleHmeDeactivate[2]) };
  const oauthReconnect = url.pathname.match(/^\/api\/accounts\/([^/]+)\/oauth\/reconnect$/);
  if (oauthReconnect && method === 'POST' && options.body === undefined) return { operation: 'oauthReconnect', accountId: decodeURIComponent(oauthReconnect[1]) };
  if (method === 'POST' && url.pathname === '/api/oauth/start' && body) return { operation: 'oauthStart', input: body };
  if (method === 'POST' && url.pathname === '/api/oauth/status' && body) return { operation: 'oauthStatus', input: body };
  if (method === 'POST' && url.pathname === '/api/send' && body) return { operation: 'messageSend', input: body };
  if (method === 'POST' && url.pathname === '/api/outbox' && body) return { operation: 'outboxSchedule', input: body };
  const outboxRetry = url.pathname.match(/^\/api\/outbox\/([^/]+)\/retry$/);
  if (outboxRetry && method === 'POST' && options.body === undefined) return { operation: 'outboxRetry', itemId: decodeURIComponent(outboxRetry[1]) };
  const outboxResolve = url.pathname.match(/^\/api\/outbox\/([^/]+)\/resolve$/);
  if (outboxResolve && method === 'POST' && body) return { operation: 'outboxResolve', itemId: decodeURIComponent(outboxResolve[1]), input: body };
  const outboxItem = url.pathname.match(/^\/api\/outbox\/([^/]+)$/);
  if (outboxItem && method === 'DELETE' && options.body === undefined) return { operation: 'outboxCancel', itemId: decodeURIComponent(outboxItem[1]) };
  const workItem = url.pathname.match(/^\/api\/messages\/([^/]+)\/work-item$/);
  if (workItem && method === 'PUT' && body) return { operation: 'mailWorkItemSet', messageId: decodeURIComponent(workItem[1]), input: body };
  if (workItem && method === 'DELETE' && options.body === undefined) return { operation: 'mailWorkItemComplete', messageId: decodeURIComponent(workItem[1]) };
  const replyDraft = url.pathname.match(/^\/api\/messages\/([^/]+)\/reply-draft$/);
  if (replyDraft && method === 'POST' && body) return { operation: 'mailReplyDraftCreate', messageId: decodeURIComponent(replyDraft[1]), input: body };
  const draftSchedule = url.pathname.match(/^\/api\/drafts\/([^/]+)\/schedule$/);
  if (draftSchedule && method === 'POST' && body) return { operation: 'mailDraftSchedule', draftId: decodeURIComponent(draftSchedule[1]), input: body };
  if (method === 'POST' && url.pathname === '/api/drafts' && body) {
    const draftId = new Headers(options.headers).get('X-Draft-Id') ?? undefined;
    return { operation: 'draftCreate', ...(draftId ? { draftId } : {}), input: body };
  }
  const draft = url.pathname.match(/^\/api\/drafts\/([^/]+)$/);
  if (draft && method === 'PUT' && body) return { operation: 'draftUpdate', draftId: decodeURIComponent(draft[1]), input: body };
  if (draft && method === 'DELETE' && options.body === undefined) return { operation: 'draftDelete', draftId: decodeURIComponent(draft[1]) };
  if (method === 'PATCH' && url.pathname === '/api/preferences' && body) return { operation: 'preferencesUpdate', input: body };
  if (method === 'POST' && url.pathname === '/api/smart-folders' && body) return { operation: 'smartFolderCreate', input: body };
  if (method === 'POST' && body) {
    if (url.pathname === '/api/mail-rules') return { operation: 'mailRuleCreate', input: body };
    if (url.pathname === '/api/mail-rules/preview') return { operation: 'mailRulePreview', input: body };
    if (url.pathname === '/api/mail-rules/apply') return { operation: 'mailRuleApply', input: body };
  }
  const mailRule = url.pathname.match(/^\/api\/mail-rules\/([^/]+)$/);
  if (mailRule && method === 'GET' && options.body === undefined) return { operation: 'mailRuleGet', ruleId: decodeURIComponent(mailRule[1]) };
  if (mailRule && method === 'PUT' && body) return { operation: 'mailRuleUpdate', ruleId: decodeURIComponent(mailRule[1]), input: body };
  if (mailRule && method === 'DELETE' && options.body === undefined) return { operation: 'mailRuleDelete', ruleId: decodeURIComponent(mailRule[1]) };
  const mailRuleEnabled = url.pathname.match(/^\/api\/mail-rules\/([^/]+)\/enabled$/);
  if (mailRuleEnabled && method === 'PATCH' && body) return { operation: 'mailRuleSetEnabled', ruleId: decodeURIComponent(mailRuleEnabled[1]), input: body };
  const ruleRetry = url.pathname.match(/^\/api\/mail-rule-runs\/([^/]+)\/retry$/);
  if (ruleRetry && method === 'POST') return { operation: 'mailRuleRetry', runId: decodeURIComponent(ruleRetry[1]) };
  const smartFolder = url.pathname.match(/^\/api\/smart-folders\/([^/]+)$/);
  if (smartFolder && method === 'PUT' && body) return { operation: 'smartFolderUpdate', folderId: decodeURIComponent(smartFolder[1]), input: body };
  if (smartFolder && method === 'DELETE' && options.body === undefined) return { operation: 'smartFolderDelete', folderId: decodeURIComponent(smartFolder[1]) };
  if (method === 'PUT' && url.pathname === '/api/translation-settings' && body) return { operation: 'translationSettingsUpdate', input: body };
  const translationProfile = url.pathname.match(/^\/api\/translation-profiles\/([^/]+)$/);
  if (translationProfile && method === 'PUT' && body) return { operation: 'translationProfileUpsert', profileId: decodeURIComponent(translationProfile[1]), input: body };
  if (translationProfile && method === 'DELETE' && options.body === undefined) return { operation: 'translationProfileDelete', profileId: decodeURIComponent(translationProfile[1]) };
  const translationCredential = url.pathname.match(/^\/api\/translation-profiles\/([^/]+)\/credential$/);
  if (translationCredential && method === 'PUT' && body) return { operation: 'translationCredentialUpdate', profileId: decodeURIComponent(translationCredential[1]), input: body };
  if (translationCredential && method === 'DELETE' && options.body === undefined) return { operation: 'translationCredentialClear', profileId: decodeURIComponent(translationCredential[1]) };
  const translationConsent = url.pathname.match(/^\/api\/translation-profiles\/([^/]+)\/consent$/);
  if (translationConsent && method === 'POST' && options.body === undefined) return { operation: 'translationConsentAccept', profileId: decodeURIComponent(translationConsent[1]) };
  if (translationConsent && method === 'DELETE' && options.body === undefined) return { operation: 'translationConsentRevoke', profileId: decodeURIComponent(translationConsent[1]) };
  const translationPrepare = url.pathname.match(/^\/api\/messages\/([^/]+)\/translations\/prepare$/);
  if (translationPrepare && method === 'POST' && body) return { operation: 'translationPrepare', messageId: decodeURIComponent(translationPrepare[1]), input: body };
  const translationExecute = url.pathname.match(/^\/api\/messages\/([^/]+)\/translations\/run$/);
  if (translationExecute && method === 'POST' && body) return { operation: 'translationExecute', messageId: decodeURIComponent(translationExecute[1]), input: body };
  const translationComplete = url.pathname.match(/^\/api\/messages\/([^/]+)\/translations\/complete$/);
  if (translationComplete && method === 'POST' && body) return { operation: 'translationComplete', messageId: decodeURIComponent(translationComplete[1]), input: body };
  if (method === 'DELETE' && options.body === undefined && url.pathname === '/api/translation-cache') return { operation: 'translationCacheClear' };
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
      throw new Error('iMail 只能处理 JSON 格式的请求内容');
    }
    const call = embeddedDomainCall(path, options);
    if (call) return invokeDesktopWithAbort<DesktopHttpResponse>(this.invoker, 'desktop_mail_service_call', { call }, options.signal, 'desktop_cancel_mail_service_call');
    throw new Error('iMail 暂时无法完成这项操作，请更新应用后重试');
  }
}
