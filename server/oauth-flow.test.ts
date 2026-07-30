import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AccountSecret, MailAccount, StoreData } from './types.js';

const state = vi.hoisted(() => ({
  store: { accounts: [], messages: [], tokens: [] } as StoreData,
  validationError: null as Error | null,
  jwtPayload: {} as Record<string, unknown>,
  fetchHandler: null as null | ((input: string | URL | Request, init?: RequestInit) => Promise<Response>),
}));

vi.mock('./store.js', () => ({
  readStore: vi.fn(async () => state.store),
  updateStore: vi.fn(async (updater: (data: StoreData) => unknown) => updater(state.store)),
  setAccountSyncStatus: vi.fn(async (accountId: string, status: MailAccount['status'], lastError?: string) => {
    const account = state.store.accounts.find((item) => item.id === accountId);
    if (account) { account.status = status; account.lastError = lastError; }
  }),
  setAccountEncryptedSecret: vi.fn(async (accountId: string, encryptedSecret: string) => {
    const account = state.store.accounts.find((item) => item.id === accountId);
    if (account) account.encryptedSecret = encryptedSecret;
  }),
}));

vi.mock('./crypto.js', () => ({
  encryptSecret: vi.fn(async (secret: AccountSecret) => JSON.stringify(secret)),
  decryptSecret: vi.fn(async (encrypted: string) => JSON.parse(encrypted) as AccountSecret),
  encryptPayload: vi.fn(async (value: unknown) => Buffer.from(JSON.stringify(value)).toString('base64url') + '.tag.data'),
  decryptPayload: vi.fn(async <T>(encrypted: string) => JSON.parse(Buffer.from(encrypted.split('.')[0], 'base64url').toString()) as T),
}));

vi.mock('./mail.js', () => ({
  testAccount: vi.fn(async () => {
    if (state.validationError) throw state.validationError;
  }),
}));

vi.mock('jose', () => ({
  createRemoteJWKSet: vi.fn(() => 'mock-jwks'),
  jwtVerify: vi.fn(async () => ({ payload: state.jwtPayload })),
}));

import { beginOAuth, completeOAuth, resolveAccountSecret } from './oauth.js';

const managedEnvironment = [
  'GOOGLE_OAUTH_CLIENT_ID', 'GOOGLE_OAUTH_CLIENT_SECRET',
  'MICROSOFT_OAUTH_CLIENT_ID', 'MICROSOFT_OAUTH_CLIENT_SECRET',
  'YAHOO_OAUTH_CLIENT_ID', 'YAHOO_OAUTH_CLIENT_SECRET', 'YAHOO_MAIL_OAUTH_APPROVED',
] as const;

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

function account(overrides: Partial<MailAccount> = {}): MailAccount {
  return {
    id: 'account-refresh', provider: 'gmail', email: 'refresh@example.com', displayName: 'Refresh',
    group: '个人', color: '#168f78', authMethod: 'oauth2', createdAt: new Date().toISOString(), status: 'connected',
    encryptedSecret: JSON.stringify({ authType: 'oauth2', oauthProvider: 'google', accessToken: 'expired', refreshToken: 'refresh-old', expiresAt: '2020-01-01T00:00:00.000Z' }),
    settings: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
    ...overrides,
  };
}

beforeEach(() => {
  state.store = { accounts: [], messages: [], tokens: [] };
  state.validationError = null;
  state.jwtPayload = {};
  state.fetchHandler = null;
  for (const key of managedEnvironment) delete process.env[key];
  vi.stubGlobal('fetch', vi.fn((input: string | URL | Request, init?: RequestInit) => {
    if (!state.fetchHandler) throw new Error('Unexpected fetch');
    return state.fetchHandler(input, init);
  }));
});

describe('complete OAuth provider flows', () => {
  it('exchanges Google code, reads identity, stores refresh token and makes duplicate callbacks idempotent', async () => {
    process.env.GOOGLE_OAUTH_CLIENT_ID = 'google-client';
    process.env.GOOGLE_OAUTH_CLIENT_SECRET = 'google-secret';
    const started = await beginOAuth({ provider: 'gmail', displayName: 'Personal Gmail', group: '个人' });
    const requests: Array<{ url: string; init?: RequestInit }> = [];
    state.fetchHandler = async (input, init) => {
      const url = String(input); requests.push({ url, init });
      if (url.includes('/token')) return json({ access_token: 'google-access', refresh_token: 'google-refresh', expires_in: 3600, token_type: 'Bearer' });
      return json({ email: 'owner@gmail.com', name: 'Owner' });
    };

    const connected = await completeOAuth({ providerKey: 'google', state: started.state, code: 'google-code' });
    expect(connected).toMatchObject({ provider: 'gmail', email: 'owner@gmail.com', displayName: 'Personal Gmail', status: 'connected' });
    expect(JSON.parse(state.store.accounts[0].encryptedSecret)).toMatchObject({ accessToken: 'google-access', refreshToken: 'google-refresh', oauthProvider: 'google' });
    expect(String(requests[0].init?.body)).toContain('code_verifier=');
    expect(requests[1].init?.headers).toMatchObject({ Authorization: 'Bearer google-access' });

    const duplicate = await completeOAuth({ providerKey: 'google', state: started.state, error: 'server_error' });
    expect(duplicate.id).toBe(connected.id);
    expect(state.store.accounts).toHaveLength(1);
  });

  it('keeps a newly authorized account and refresh token when IMAP validation is temporarily unavailable', async () => {
    process.env.GOOGLE_OAUTH_CLIENT_ID = 'google-client';
    process.env.GOOGLE_OAUTH_CLIENT_SECRET = 'google-secret';
    const started = await beginOAuth({ provider: 'gmail' });
    state.validationError = new Error('IMAP 验证失败：服务暂时不可用');
    state.fetchHandler = async (input) => String(input).includes('/token')
      ? json({ access_token: 'saved-access', refresh_token: 'saved-refresh', expires_in: 3600 })
      : json({ email: 'recoverable@gmail.com' });

    const result = await completeOAuth({ providerKey: 'google', state: started.state, code: 'code' });
    expect(result).toMatchObject({ status: 'error', lastError: 'IMAP 验证失败：服务暂时不可用' });
    expect(state.store.accounts[0]).toMatchObject({ status: 'error', lastError: 'IMAP 验证失败：服务暂时不可用' });
    expect(JSON.parse(state.store.accounts[0].encryptedSecret).refreshToken).toBe('saved-refresh');
  });

  it('completes Microsoft consumer OAuth for Hotmail with verified ID-token identity', async () => {
    process.env.MICROSOFT_OAUTH_CLIENT_ID = 'microsoft-client';
    process.env.MICROSOFT_OAUTH_CLIENT_SECRET = 'microsoft-secret';
    const started = await beginOAuth({ provider: 'hotmail', group: '工作' });
    const authorize = new URL(started.authorizationUrl);
    state.jwtPayload = {
      nonce: authorize.searchParams.get('nonce'),
      iss: 'https://login.microsoftonline.com/consumers/v2.0',
      preferred_username: 'owner@hotmail.com',
      name: 'Hotmail Owner',
    };
    state.fetchHandler = async (input) => {
      expect(String(input)).toContain('/consumers/oauth2/v2.0/token');
      return json({ access_token: 'microsoft-access', refresh_token: 'microsoft-refresh', expires_in: 3600, id_token: 'signed-id-token' });
    };

    const connected = await completeOAuth({ providerKey: 'microsoft', state: started.state, code: 'microsoft-code' });
    expect(connected).toMatchObject({ provider: 'hotmail', email: 'owner@hotmail.com', displayName: 'Hotmail Owner', status: 'connected' });
    expect(JSON.parse(connected.encryptedSecret)).toMatchObject({ oauthProvider: 'microsoft', refreshToken: 'microsoft-refresh' });
  });

  it('completes approved Yahoo mail OAuth and uses Basic client authentication for token exchange', async () => {
    process.env.YAHOO_OAUTH_CLIENT_ID = 'yahoo-client';
    process.env.YAHOO_OAUTH_CLIENT_SECRET = 'yahoo-secret';
    process.env.YAHOO_MAIL_OAUTH_APPROVED = 'true';
    const started = await beginOAuth({ provider: 'yahoo' });
    let tokenAuthorization = '';
    state.fetchHandler = async (input, init) => {
      if (String(input).includes('get_token')) {
        tokenAuthorization = (init?.headers as Record<string, string>).Authorization;
        return json({ access_token: 'yahoo-access', refresh_token: 'yahoo-refresh', expires_in: 3600, scope: 'openid email profile mail-r mail-w' });
      }
      return json({ email: 'owner@yahoo.com', name: 'Yahoo Owner' });
    };

    const connected = await completeOAuth({ providerKey: 'yahoo', state: started.state, code: 'yahoo-code' });
    expect(connected).toMatchObject({ provider: 'yahoo', email: 'owner@yahoo.com', status: 'connected' });
    expect(tokenAuthorization).toBe(`Basic ${Buffer.from('yahoo-client:yahoo-secret').toString('base64')}`);
    expect(JSON.parse(connected.encryptedSecret).scopes).toEqual(expect.arrayContaining(['mail-r', 'mail-w']));
  });

  it('coalesces concurrent refreshes and persists the rotated access token', async () => {
    process.env.GOOGLE_OAUTH_CLIENT_ID = 'google-client';
    process.env.GOOGLE_OAUTH_CLIENT_SECRET = 'google-secret';
    const configured = account();
    state.store.accounts = [configured];
    state.fetchHandler = async () => json({ access_token: 'refreshed-access', expires_in: 3600 });

    const [first, second] = await Promise.all([resolveAccountSecret(configured), resolveAccountSecret(configured)]);
    expect(first.accessToken).toBe('refreshed-access');
    expect(second.accessToken).toBe('refreshed-access');
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(JSON.parse(state.store.accounts[0].encryptedSecret)).toMatchObject({ accessToken: 'refreshed-access', refreshToken: 'refresh-old' });
  });
});
