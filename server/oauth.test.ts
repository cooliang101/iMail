import { afterEach, describe, expect, it } from 'vitest';
import { beginOAuth, beginOAuthReconnect, completeOAuth, oauthCallbackHtml, oauthProviderCatalog } from './oauth.js';
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
  it('builds Google authorization code + PKCE request with mail and offline access', () => {
    process.env.GOOGLE_OAUTH_CLIENT_ID = 'google-client';
    process.env.GOOGLE_OAUTH_CLIENT_SECRET = 'google-secret';
    const result = beginOAuth({ provider: 'gmail', group: '个人' });
    const url = new URL(result.authorizationUrl);
    expect(url.origin).toBe('https://accounts.google.com');
    expect(url.searchParams.get('response_type')).toBe('code');
    expect(url.searchParams.get('code_challenge_method')).toBe('S256');
    expect(url.searchParams.get('access_type')).toBe('offline');
    expect(url.searchParams.get('scope')).toContain('https://mail.google.com/');
    expect(url.searchParams.get('state')).toHaveLength(43);
  });

  it('uses Microsoft IMAP, SMTP and refresh scopes for Outlook', () => {
    process.env.MICROSOFT_OAUTH_CLIENT_ID = 'microsoft-client';
    const result = beginOAuth({ provider: 'outlook' });
    const scope = new URL(result.authorizationUrl).searchParams.get('scope') ?? '';
    expect(scope).toContain('IMAP.AccessAsUser.All');
    expect(scope).toContain('SMTP.Send');
    expect(scope).toContain('offline_access');
  });

  it('does not advertise Yahoo mail OAuth as configured before restricted-scope approval', () => {
    process.env.YAHOO_OAUTH_CLIENT_ID = 'yahoo-client';
    process.env.YAHOO_OAUTH_CLIENT_SECRET = 'yahoo-secret';
    expect(oauthProviderCatalog().find((item) => item.id === 'yahoo')?.configured).toBe(false);
    process.env.YAHOO_MAIL_OAUTH_APPROVED = 'true';
    expect(oauthProviderCatalog().find((item) => item.id === 'yahoo')?.configured).toBe(true);
  });

  it('rejects a fake QQ OAuth flow because no public mail OAuth exists', () => {
    expect(() => beginOAuth({ provider: 'qq' })).toThrow('没有公开可用的邮件 OAuth 接口');
  });

  it('builds a reconnect request only for an existing OAuth account', () => {
    process.env.GOOGLE_OAUTH_CLIENT_ID = 'google-client';
    process.env.GOOGLE_OAUTH_CLIENT_SECRET = 'google-secret';
    const account = {
      id: 'account-1', provider: 'gmail', email: 'owner@example.com', displayName: 'Owner',
      group: '个人', color: '#168f78', authMethod: 'oauth2', encryptedSecret: 'encrypted',
      settings: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
      createdAt: new Date().toISOString(), status: 'connected',
    } satisfies MailAccount;
    expect(new URL(beginOAuthReconnect(account).authorizationUrl).searchParams.get('prompt')).toContain('consent');
    expect(() => beginOAuthReconnect({ ...account, authMethod: 'app-password' })).toThrow('不是通过 OAuth 接入');
  });

  it('rejects missing or replayed callback state before token exchange', async () => {
    await expect(completeOAuth({ providerKey: 'google', state: 'unknown', code: 'code' })).rejects.toThrow('state 无效或已过期');
  });

  it('escapes callback content before rendering it into the popup page', () => {
    const html = oauthCallbackHtml({ success: false, message: '<script>alert(1)</script>' });
    expect(html).not.toContain('<script>alert(1)</script>');
    expect(html).toContain('scriptalert(1)/script');
  });
});
