import { describe, expect, it, vi } from 'vitest';
import { createMailService, embeddedDomainCall, HttpMailService, TauriMailService } from './index';
import type { DesktopHttpInvoker } from '../desktop/http';

describe('mail service client adapters', () => {
  it('maps local embedded requests to a typed Tauri command without a service URL or cookie transport', async () => {
    const invokeMock = vi.fn(async (_command: string, _args?: Record<string, unknown>) => ({ status: 200, body: '{"accounts":[]}' }));
    const invoker: DesktopHttpInvoker = async <T>(command: string, args?: Record<string, unknown>) => invokeMock(command, args) as Promise<T>;
    const service = new TauriMailService(invoker);
    await expect(service.request('/api/accounts/account-1', { method: 'PATCH', body: '{"color":"#fff"}' }))
      .resolves.toEqual({ status: 200, body: '{"accounts":[]}' });
    expect(invokeMock).toHaveBeenCalledWith('desktop_mail_service_call', { call: {
      operation: 'accountUpdate', accountId: 'account-1', input: { color: '#fff' },
    } });
    expect(JSON.stringify(invokeMock.mock.calls[0])).not.toContain('baseUrl');
  });

  it('forwards cancellation to the embedded Rust service', async () => {
    let finishRequest: ((value: { status: number; body: string }) => void) | undefined;
    const invokeMock = vi.fn((command: string, _args?: Record<string, unknown>) => command === 'desktop_mail_service_call'
      ? new Promise((resolve) => { finishRequest = resolve; })
      : Promise.resolve(false));
    const controller = new AbortController();
    const request = new TauriMailService(invokeMock as DesktopHttpInvoker).request('/api/messages?limit=10', { signal: controller.signal });

    controller.abort();
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
    const requestId = (invokeMock.mock.calls[0][1] as { requestId: string }).requestId;
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'desktop_cancel_mail_service_call', { requestId });
    finishRequest?.({ status: 200, body: '{}' });
    await expect(request).rejects.toMatchObject({ name: 'AbortError' });
  });

  it('keeps web and remote desktop selections on the HTTP adapter', () => {
    const requester = vi.fn();
    expect(createMailService({ tauri: false, mode: 'remote', embedded: true, httpRequester: requester })).toBeInstanceOf(HttpMailService);
    expect(createMailService({ tauri: true, mode: 'remote', embedded: true, httpRequester: requester })).toBeInstanceOf(HttpMailService);
    expect(createMailService({ tauri: true, mode: 'local', embedded: false, httpRequester: requester })).toBeInstanceOf(HttpMailService);
    expect(createMailService({ tauri: true, mode: 'local', embedded: true, invoker: vi.fn() })).toBeInstanceOf(TauriMailService);
  });

  it('rejects non-JSON body objects before they cross the WebView boundary', () => {
    const service = new TauriMailService(vi.fn());
    expect(() => service.request('/api/messages', { method: 'POST', body: new Uint8Array([1]) as unknown as BodyInit }))
      .toThrow('JSON');
  });

  it('maps first-wave read operations to typed Rust domain calls', async () => {
    expect(embeddedDomainCall('/api/auth/status')).toEqual({ operation: 'authStatus' });
    expect(embeddedDomainCall('/api/accounts')).toEqual({ operation: 'accountsList' });
    expect(embeddedDomainCall('/api/providers')).toEqual({ operation: 'providers' });
    expect(embeddedDomainCall('/api/message-stats')).toEqual({ operation: 'messageStats' });
    expect(embeddedDomainCall('/api/messages?limit=60&offset=0&search=hello%20world')).toEqual({
      operation: 'messagesList', query: { limit: '60', offset: '0', search: 'hello world' },
    });
    expect(embeddedDomainCall('/api/messages?sender=Sender%2BAlerts%40Example.com&recipient=alias%40icloud.com')).toEqual({
      operation: 'messagesList', query: { sender: 'Sender+Alerts@Example.com', recipient: 'alias@icloud.com' },
    });
    expect(embeddedDomainCall('/api/messages/id%20with%20space')).toEqual({ operation: 'messageDetail', messageId: 'id with space' });
    expect(embeddedDomainCall('/api/messages', { method: 'POST', body: '{}' })).toBeNull();

    const invokeMock = vi.fn(async () => ({ status: 200, body: '{}' }));
    const service = new TauriMailService(invokeMock as DesktopHttpInvoker);
    await service.request('/api/messages?limit=10');
    expect(invokeMock).toHaveBeenCalledWith('desktop_mail_service_call', {
      call: { operation: 'messagesList', query: { limit: '10' } },
    });
  });

  it('maps sync and message mutations to typed calls without exposing route bodies', () => {
    expect(embeddedDomainCall('/api/sync', { method: 'POST' })).toEqual({ operation: 'syncAll' });
    expect(embeddedDomainCall('/api/accounts/account%201/sync', { method: 'POST' })).toEqual({ operation: 'syncAccount', accountId: 'account 1' });
    expect(embeddedDomainCall('/api/accounts/a/mailboxes/sync', { method: 'POST', body: '{"mailbox":"Archive/2026"}' })).toEqual({ operation: 'syncAccountMailbox', accountId: 'a', mailbox: 'Archive/2026' });
    expect(embeddedDomainCall('/api/mailboxes/sent/sync', { method: 'POST' })).toEqual({ operation: 'syncMailboxRole', role: 'sent' });
    expect(embeddedDomainCall('/api/messages/m1', { method: 'PATCH', body: '{"unread":false,"labels":["work"]}' })).toEqual({ operation: 'messageUpdate', messageId: 'm1', unread: false, labels: ['work'] });
    expect(embeddedDomainCall('/api/messages/m1/move', { method: 'POST', body: '{"destination":"archive"}' })).toEqual({ operation: 'messageMove', messageId: 'm1', destination: 'archive' });
  });

  it('maps every remaining UI domain and rejects an opaque fallback route', () => {
    const mapped: Array<[string, RequestInit | undefined, string]> = [
      ['/api/auth/register', { method: 'POST', body: '{"login":"owner"}' }, 'authRegister'],
      ['/api/auth/login', { method: 'POST', body: '{"login":"owner"}' }, 'authLogin'],
      ['/api/auth/logout', { method: 'POST' }, 'authLogout'],
      ['/api/accounts', { method: 'POST', body: '{"provider":"custom"}' }, 'accountCreate'],
      ['/api/accounts/a', { method: 'DELETE' }, 'accountDelete'],
      ['/api/accounts/a/credential', { method: 'PUT', body: '{"password":"secret"}' }, 'accountCredentialUpdate'],
      ['/api/accounts/a/proxy', { method: 'PUT', body: '{"enabled":false}' }, 'accountProxyUpdate'],
      ['/api/accounts/a/connection-test', { method: 'POST' }, 'accountConnectionTest'],
      ['/api/accounts/a/apple-hme', undefined, 'appleHmeStatus'],
      ['/api/accounts/a/apple-hme', { method: 'DELETE' }, 'appleHmeDisconnect'],
      ['/api/accounts/a/apple-hme/login', { method: 'POST', body: '{"kind":"appleAccount"}' }, 'appleHmeStartLogin'],
      ['/api/accounts/a/apple-hme/two-factor', { method: 'POST', body: '{"code":"123456"}' }, 'appleHmeSubmitTwoFactor'],
      ['/api/accounts/a/apple-hme/addresses', undefined, 'appleHmeList'],
      ['/api/accounts/a/apple-hme/addresses/sync', { method: 'POST' }, 'appleHmeSync'],
      ['/api/accounts/a/apple-hme/addresses', { method: 'POST', body: '{"label":"购物"}' }, 'appleHmeCreate'],
      ['/api/accounts/a/apple-hme/addresses/h1/deactivate', { method: 'POST' }, 'appleHmeDeactivate'],
      ['/api/accounts/a/apple-hme/addresses/h1', { method: 'DELETE' }, 'appleHmeDelete'],
      ['/api/oauth/start', { method: 'POST', body: '{"provider":"google"}' }, 'oauthStart'],
      ['/api/accounts/a/oauth/reconnect', { method: 'POST' }, 'oauthReconnect'],
      ['/api/oauth/status', { method: 'POST', body: '{"state":"opaque"}' }, 'oauthStatus'],
      ['/api/send', { method: 'POST', body: '{"subject":"hello"}' }, 'messageSend'],
      ['/api/messages/m1/attachments/2/preview', { method: 'POST' }, 'attachmentPreviewCreate'],
      ['/api/attachment-previews/preview-1', { method: 'DELETE' }, 'attachmentPreviewDelete'],
      ['/api/drafts', undefined, 'draftsList'],
      ['/api/drafts', { method: 'POST', headers: { 'X-Draft-Id': 'draft-1' }, body: '{"subject":"draft"}' }, 'draftCreate'],
      ['/api/drafts/draft-1', { method: 'PUT', body: '{"subject":"draft"}' }, 'draftUpdate'],
      ['/api/drafts/draft-1', { method: 'DELETE' }, 'draftDelete'],
      ['/api/preferences', undefined, 'preferencesGet'],
      ['/api/preferences', { method: 'PATCH', body: '{"theme":"light"}' }, 'preferencesUpdate'],
      ['/api/translation-settings', undefined, 'translationSettingsGet'],
      ['/api/translation-settings', { method: 'PUT', body: '{"preferences":{},"environment":{}}' }, 'translationSettingsUpdate'],
      ['/api/translation-profiles/deepl-1', { method: 'PUT', body: '{"displayName":"DeepL"}' }, 'translationProfileUpsert'],
      ['/api/translation-profiles/deepl-1', { method: 'DELETE' }, 'translationProfileDelete'],
      ['/api/translation-profiles/deepl-1/credential', { method: 'PUT', body: '{"kind":"deepl-api-key","secret":"x"}' }, 'translationCredentialUpdate'],
      ['/api/translation-profiles/deepl-1/credential', { method: 'DELETE' }, 'translationCredentialClear'],
      ['/api/translation-profiles/deepl-1/consent', { method: 'POST' }, 'translationConsentAccept'],
      ['/api/translation-profiles/deepl-1/consent', { method: 'DELETE' }, 'translationConsentRevoke'],
      ['/api/messages/message%201/translations/prepare', { method: 'POST', body: '{"profileId":"edge","targetLanguage":"zh-Hans"}' }, 'translationPrepare'],
      ['/api/messages/message%201/translations/run', { method: 'POST', body: '{"profileId":"deepl","targetLanguage":"zh-Hans"}' }, 'translationExecute'],
      ['/api/messages/message%201/translations/complete', { method: 'POST', body: '{"profileId":"edge","targetLanguage":"zh-Hans","segments":[]}' }, 'translationComplete'],
      ['/api/translation-cache', { method: 'DELETE' }, 'translationCacheClear'],
      ['/api/developer-tokens', undefined, 'developerTokensList'],
      ['/api/developer-tokens', { method: 'POST', body: '{"name":"cli"}' }, 'developerTokenCreate'],
      ['/api/developer-tokens/token-1', { method: 'DELETE' }, 'developerTokenDelete'],
      ['/api/external-access', undefined, 'externalAccessGet'],
      ['/api/external-access', { method: 'PATCH', body: '{"restEnabled":true}' }, 'externalAccessUpdate'],
      ['/api/security/mail-authorization-exports', { method: 'POST', body: '{"currentPassword":"x"}' }, 'authorizationExportPrepare'],
      ['/api/security/clear-user-data', { method: 'POST', body: '{"confirmation":"x"}' }, 'userDataClear'],
    ];
    for (const [path, options, operation] of mapped) expect(embeddedDomainCall(path, options)?.operation).toBe(operation);
    const service = new TauriMailService(vi.fn());
    expect(() => service.request('/api/unmapped-internal')).toThrow('iMail 暂时无法完成这项操作');
  });
});
