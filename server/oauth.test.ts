import { afterEach, describe, expect, it } from 'vitest';
import { beginOAuth, beginOAuthReconnect, completeOAuth, describeOAuthCallbackError, oauthCallbackHtml, oauthProviderCatalog } from './oauth.js';
import type { MailAccount } from './types.js';

const managedEnvironment = [
  'GOOGLE_OAUTH_CLIENT_ID', 'GOOGLE_OAUTH_CLIENT_SECRET',
  'MICROSOFT_OAUTH_CLIENT_ID', 'MICROSOFT_OAUTH_CLIENT_SECRET',
  'YAHOO_OAUTH_CLIENT_ID', 'YAHOO_OAUTH_CLIENT_SECRET', 'YAHOO_MAIL_OAUTH_APPROVED',
] as const;

afterEach(() => {
  for (const key of managedEnvironment) delete process.env[key];
});

describe('OAuth authorization', () => {
  it('builds Google authorization code + PKCE request with mail and offline access', async () => {
    process.env.GOOGLE_OAUTH_CLIENT_ID = 'google-client';
    process.env.GOOGLE_OAUTH_CLIENT_SECRET = 'google-secret';
    const result = await beginOAuth({ provider: 'gmail', group: '个人' });
    const url = new URL(result.authorizationUrl);
    expect(url.origin).toBe('https://accounts.google.com');
    expect(url.searchParams.get('response_type')).toBe('code');
    expect(url.searchParams.get('code_challenge_method')).toBe('S256');
    expect(url.searchParams.get('access_type')).toBe('offline');
    expect(url.searchParams.get('scope')).toContain('https://mail.google.com/');
    expect((url.searchParams.get('state') ?? '').split('.')).toHaveLength(3);
  });

  it('uses Microsoft IMAP, SMTP and refresh scopes for Outlook', async () => {
    process.env.MICROSOFT_OAUTH_CLIENT_ID = 'microsoft-client';
    const result = await beginOAuth({ provider: 'outlook' });
    const scope = new URL(result.authorizationUrl).searchParams.get('scope') ?? '';
    expect(scope).toContain('IMAP.AccessAsUser.All');
    expect(scope).toContain('SMTP.Send');
    expect(scope).toContain('offline_access');
  });

  it('uses the Microsoft consumer tenant for Hotmail and Outlook.com accounts', async () => {
    process.env.MICROSOFT_OAUTH_CLIENT_ID = 'microsoft-client';
    const result = await beginOAuth({ provider: 'hotmail' });
    expect(new URL(result.authorizationUrl).pathname).toBe('/consumers/oauth2/v2.0/authorize');
  });

  it('does not advertise Yahoo mail OAuth as configured before restricted-scope approval', () => {
    process.env.YAHOO_OAUTH_CLIENT_ID = 'yahoo-client';
    process.env.YAHOO_OAUTH_CLIENT_SECRET = 'yahoo-secret';
    expect(oauthProviderCatalog().find((item) => item.id === 'yahoo')?.configured).toBe(false);
    process.env.YAHOO_MAIL_OAUTH_APPROVED = 'true';
    expect(oauthProviderCatalog().find((item) => item.id === 'yahoo')?.configured).toBe(true);
  });

  it('builds Yahoo authorization code + PKCE only after mail scope approval', async () => {
    process.env.YAHOO_OAUTH_CLIENT_ID = 'yahoo-client';
    process.env.YAHOO_OAUTH_CLIENT_SECRET = 'yahoo-secret';
    process.env.YAHOO_MAIL_OAUTH_APPROVED = 'true';
    const result = await beginOAuth({ provider: 'yahoo' });
    const url = new URL(result.authorizationUrl);
    expect(url.origin).toBe('https://api.login.yahoo.com');
    expect(url.searchParams.get('scope')).toContain('mail-r');
    expect(url.searchParams.get('scope')).toContain('mail-w');
    expect(url.searchParams.get('code_challenge_method')).toBe('S256');
  });

  it('rejects a fake QQ OAuth flow because no public mail OAuth exists', async () => {
    await expect(beginOAuth({ provider: 'qq' })).rejects.toThrow('没有公开可用的邮件 OAuth 接口');
  });

  it('builds a reconnect request only for an existing OAuth account', async () => {
    process.env.GOOGLE_OAUTH_CLIENT_ID = 'google-client';
    process.env.GOOGLE_OAUTH_CLIENT_SECRET = 'google-secret';
    const account = {
      id: 'account-1', provider: 'gmail', email: 'owner@example.com', displayName: 'Owner',
      group: '个人', color: '#168f78', authMethod: 'oauth2', encryptedSecret: 'encrypted',
      settings: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
      createdAt: new Date().toISOString(), status: 'connected',
    } satisfies MailAccount;
    expect(new URL((await beginOAuthReconnect(account)).authorizationUrl).searchParams.get('prompt')).toContain('consent');
    await expect(beginOAuthReconnect({ ...account, authMethod: 'app-password' })).rejects.toThrow('不是通过 OAuth 接入');
  });

  it('rejects missing or replayed callback state before token exchange', async () => {
    await expect(completeOAuth({ providerKey: 'google', state: 'unknown', code: 'code' })).rejects.toThrow('state 无效或已过期');
  });

  it('escapes callback content before rendering it into the popup page', () => {
    const html = oauthCallbackHtml({ success: false, message: '<script>alert(1)</script>' });
    expect(html).not.toContain('<script>alert(1)</script>');
    expect(html).toContain('scriptalert(1)/script');
  });

  it('returns a successful callback with a warning when authorization is saved but mail validation fails', () => {
    const html = oauthCallbackHtml({ success: true, accountId: 'account-1', message: '授权已保存', warning: 'IMAP 验证失败' });
    expect(html).toContain('授权已保存');
    expect(html).toContain('"success":true');
    expect(html).toContain('"warning":"IMAP 验证失败"');
  });

  it('turns provider callback codes into actionable messages while preserving detailed diagnostics', () => {
    expect(describeOAuthCallbackError('access_denied')).toContain('取消或拒绝');
    expect(describeOAuthCallbackError('server_error')).toContain('暂时未能完成');
    expect(describeOAuthCallbackError('unauthorized_client')).toContain('配置不正确');
    expect(describeOAuthCallbackError('server_error', 'AADSTS70012: MSA server error')).toBe('AADSTS70012: MSA server error');
  });
});
