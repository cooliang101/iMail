import { mkdtemp, rm } from 'node:fs/promises';
import { createDecipheriv, scrypt } from 'node:crypto';
import type { Server } from 'node:http';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MailAccount } from './types.js';

vi.mock('./mail.js', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./mail.js')>();
  return {
    ...actual,
    updateRemoteMessageFlags: vi.fn(async () => undefined),
    moveRemoteMessage: vi.fn(async (_messageId: string, destination: 'archive' | 'trash') => ({ mailbox: destination === 'archive' ? 'Archive' : 'Trash' })),
    downloadAttachment: vi.fn(async () => ({ content: Buffer.from('hello'), filename: 'report.txt', contentType: 'text/plain' })),
  };
});

let directory: string;
let server: Server;
let baseUrl: string;
let updateStore: typeof import('./store.js')['updateStore'];
let updateAppStore: typeof import('./store.js')['updateStore'];
let closeStore: typeof import('./store.js')['closeStore'];
let getSyncStore: typeof import('./sync/store.js')['getSyncStore'];
let withUserContext: typeof import('./auth/context.js')['withUserContext'];
let authCookie = '';
let appUserId = '';
let privacyOtherCookie = '';
let privacyOtherUserId = '';

const account: MailAccount = {
  id: '11111111-1111-4111-8111-111111111111', provider: 'gmail', email: 'owner@example.com', displayName: 'Owner',
  group: '个人', color: '#168f78', settings: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
  encryptedSecret: 'must-never-leak', authMethod: 'oauth2', createdAt: '2026-07-28T00:00:00.000Z', status: 'connected',
};

beforeAll(async () => {
  directory = await mkdtemp(path.join(tmpdir(), 'imail-api-'));
  process.env.IMAIL_DATA_DIR = directory;
  process.env.APP_MASTER_KEY = '11'.repeat(32);
  vi.resetModules();
  const [{ app }, store, syncStore, authContext] = await Promise.all([import('./index.js'), import('./store.js'), import('./sync/store.js'), import('./auth/context.js')]);
  updateStore = store.updateStore;
  closeStore = store.closeStore;
  getSyncStore = syncStore.getSyncStore;
  withUserContext = authContext.withUserContext;
  updateAppStore = (mutator) => withUserContext(appUserId, () => updateStore(mutator));
  server = app.listen(0, '127.0.0.1');
  await new Promise<void>((resolve) => server.once('listening', resolve));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('测试服务器启动失败');
  baseUrl = `http://127.0.0.1:${address.port}`;
  const registered = await fetch(`${baseUrl}/api/auth/register`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ login: 'test-owner', displayName: 'Test Owner', password: 'test-password-123' }) });
  appUserId = (await registered.json()).user.id;
  authCookie = registered.headers.get('set-cookie')?.split(';')[0] ?? '';
  const privacyOther = await fetch(`${baseUrl}/api/auth/register`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ login: 'privacy-other', displayName: 'Privacy Other', password: 'privacy-password-123' }) });
  privacyOtherUserId = (await privacyOther.json()).user.id;
  privacyOtherCookie = privacyOther.headers.get('set-cookie')?.split(';')[0] ?? '';
});

afterAll(async () => {
  await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  closeStore();
  (await import('./auth/http.js')).closeAuthStore();
  delete process.env.IMAIL_DATA_DIR; delete process.env.APP_MASTER_KEY;
  await rm(directory, { recursive: true, force: true });
});

beforeEach(async () => {
  await withUserContext(appUserId, () => updateStore((data) => { data.accounts = [account]; data.messages = []; data.tokens = []; data.drafts = []; }));
  getSyncStore().deleteAccountData(account.id); getSyncStore().ensurePolicy(account.id);
  await request('/api/preferences', { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ startupView: 'inbox', markReadOnOpen: true, defaultMessageView: 'source', notificationKinds: { unread: true, snooze: true, error: true } }) });
});

async function request(route: string, init?: RequestInit) {
  const headers = new Headers(init?.headers); headers.set('Cookie', authCookie);
  const response = await fetch(`${baseUrl}${route}`, { ...init, headers });
  const body = response.status === 204 ? undefined : await response.json();
  return { response, body };
}

async function decryptAuthorizationExport(envelope: {
  format: string;
  formatVersion: number;
  kdf: { salt: string; cost: number; blockSize: number; parallelization: number; keyLength: number };
  cipher: { iv: string; authTag: string };
  ciphertext: string;
}, password: string) {
  const key = await new Promise<Buffer>((resolve, reject) => {
    scrypt(password, Buffer.from(envelope.kdf.salt, 'base64url'), envelope.kdf.keyLength, {
      N: envelope.kdf.cost,
      r: envelope.kdf.blockSize,
      p: envelope.kdf.parallelization,
      maxmem: 64 * 1024 * 1024,
    }, (error, derived) => error ? reject(error) : resolve(derived));
  });
  try {
    const decipher = createDecipheriv('aes-256-gcm', key, Buffer.from(envelope.cipher.iv, 'base64url'));
    decipher.setAAD(Buffer.from(`${envelope.format}:v${envelope.formatVersion}`, 'utf8'));
    decipher.setAuthTag(Buffer.from(envelope.cipher.authTag, 'base64url'));
    return JSON.parse(Buffer.concat([
      decipher.update(Buffer.from(envelope.ciphertext, 'base64url')),
      decipher.final(),
    ]).toString('utf8'));
  } finally { key.fill(0); }
}

describe('iMail HTTP API', () => {
  it('exposes a stable public service identity and protocol contract', async () => {
    const first = await fetch(`${baseUrl}/api/system/info`);
    const second = await fetch(`${baseUrl}/api/system/info`);
    expect(first.status).toBe(200);
    expect(first.headers.get('cache-control')).toBe('no-store');
    const firstBody = await first.json();
    const secondBody = await second.json();
    expect(firstBody).toMatchObject({
      service: 'imail', version: '0.0.1', protocolVersion: 1,
      capabilities: { gateway: true, mcp: true, syncWorker: true, webClient: false },
    });
    expect(firstBody.instanceId).toMatch(/^[0-9a-f-]{36}$/);
    expect(secondBody.instanceId).toBe(firstBody.instanceId);
  });

  it('allows the configured web client origin and issues a cross-origin secure session cookie', async () => {
    const response = await fetch(`${baseUrl}/api/auth/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Origin: 'http://localhost:5173' },
      body: JSON.stringify({ login: 'test-owner', password: 'test-password-123' }),
    });
    expect(response.status).toBe(200);
    expect(response.headers.get('access-control-allow-origin')).toBe('http://localhost:5173');
    expect(response.headers.get('access-control-allow-credentials')).toBe('true');
    expect(response.headers.get('set-cookie')).toContain('SameSite=None');
    expect(response.headers.get('set-cookie')).toContain('Secure');
  });

  it('persists validated application preferences on the server', async () => {
    const initial = await request('/api/preferences');
    expect(initial.body.preferences).toMatchObject({ theme: 'mint-fresh', startupView: 'inbox', defaultMessageView: 'source' });
    const updated = await request('/api/preferences', {
      method: 'PATCH', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ theme: 'tech', startupView: 'starred', defaultMessageView: 'rendered', notificationKinds: { snooze: false } }),
    });
    expect(updated.body.preferences).toMatchObject({ theme: 'tech', startupView: 'starred', markReadOnOpen: true, defaultMessageView: 'rendered', notificationKinds: { unread: true, snooze: false, error: true }, shortcutBindings: { focusSearch: 'Mod+K' } });
    expect((await request('/api/preferences')).body.preferences).toEqual(updated.body.preferences);
    expect((await request('/api/preferences', { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ startupView: 'invalid' }) })).response.status).toBe(400);
  });

  it('blocks unauthenticated application access and isolates another application account', async () => {
    expect((await fetch(`${baseUrl}/api/accounts`)).status).toBe(401);
    await request('/api/preferences', { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ startupView: 'starred' }) });
    const registered = await fetch(`${baseUrl}/api/auth/register`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ login: 'other-user', displayName: 'Other User', password: 'other-password-123' }),
    });
    expect(registered.status).toBe(201);
    const otherCookie = registered.headers.get('set-cookie')?.split(';')[0] ?? '';
    const isolated = await fetch(`${baseUrl}/api/accounts`, { headers: { Cookie: otherCookie } });
    expect(isolated.status).toBe(200);
    expect((await isolated.json()).accounts).toEqual([]);
    const isolatedPreferences = await fetch(`${baseUrl}/api/preferences`, { headers: { Cookie: otherCookie } });
    expect((await isolatedPreferences.json()).preferences.startupView).toBe('inbox');
    expect((await request('/api/preferences')).body.preferences.startupView).toBe('starred');
    const wrongPassword = await fetch(`${baseUrl}/api/auth/login`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ login: 'test-owner', password: 'wrong-password' }) });
    expect(wrongPassword.status).toBe(401);
  });

  it('closes registration after initial setup in production unless explicitly opened', async () => {
    const previousNodeEnv = process.env.NODE_ENV;
    const previousMode = process.env.IMAIL_REGISTRATION_MODE;
    process.env.NODE_ENV = 'production';
    delete process.env.IMAIL_REGISTRATION_MODE;
    try {
      const status = await fetch(`${baseUrl}/api/auth/status`).then((response) => response.json());
      expect(status).toMatchObject({ setupRequired: false, registrationOpen: false });
      const response = await fetch(`${baseUrl}/api/auth/register`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ login: 'closed-registration', displayName: 'Closed', password: 'test-password-123' }),
      });
      expect(response.status).toBe(403);
    } finally {
      if (previousNodeEnv === undefined) delete process.env.NODE_ENV; else process.env.NODE_ENV = previousNodeEnv;
      if (previousMode === undefined) delete process.env.IMAIL_REGISTRATION_MODE; else process.env.IMAIL_REGISTRATION_MODE = previousMode;
    }
  });

  it('rate limits repeated login attempts independently of attacker-selected IP/login pairs', async () => {
    const attempt = () => fetch(`${baseUrl}/api/auth/login`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ login: 'rate-limit-target', password: 'wrong-password' }),
    });
    for (let index = 0; index < 10; index += 1) expect((await attempt()).status).toBe(401);
    const limited = await attempt();
    expect(limited.status).toBe(429);
    expect(Number(limited.headers.get('retry-after'))).toBeGreaterThan(0);
  });

  it('persistently rate limits sensitive-action password reauthentication', async () => {
    const attempt = () => fetch(`${baseUrl}/api/security/clear-user-data`, {
      method: 'POST', headers: { 'Content-Type': 'application/json', Cookie: privacyOtherCookie },
      body: JSON.stringify({ currentPassword: 'wrong-privacy-password', confirmation: '清除我的邮箱数据' }),
    });
    for (let index = 0; index < 5; index += 1) expect((await attempt()).status).toBe(403);
    const limited = await attempt();
    expect(limited.status).toBe(429);
    expect(Number(limited.headers.get('retry-after'))).toBeGreaterThan(0);
    const audit = await fetch(`${baseUrl}/api/security/audit-events?limit=10`, { headers: { Cookie: privacyOtherCookie } }).then((response) => response.json());
    expect(audit.events).toEqual(expect.arrayContaining([
      expect.objectContaining({ eventType: 'sensitive-action.reauthentication-rate-limited', detail: { action: 'clear-user-data' } }),
    ]));
    expect(JSON.stringify(audit)).not.toContain('wrong-privacy-password');
  });

  it('exports only the current user mailbox authorization configuration in a one-time encrypted file', async () => {
    const { encryptSecret } = await import('./crypto.js');
    const appPassword = 'mail-app-password-secret';
    const proxyPassword = 'proxy-password-secret';
    const accessToken = 'oauth-access-token-secret';
    const refreshToken = 'oauth-refresh-token-secret';
    const exportPassword = 'portable-export-password';
    const oauthAccount: MailAccount = {
      ...account,
      id: '33333333-3333-4333-8333-333333333333',
      email: 'oauth-owner@example.com',
      displayName: 'OAuth Owner',
      authMethod: 'oauth2',
      createdAt: '2026-07-29T00:00:00.000Z',
      encryptedSecret: await encryptSecret({
        authType: 'oauth2', oauthProvider: 'google', accessToken, refreshToken,
        expiresAt: '2026-08-05T00:00:00.000Z', scopes: ['openid', 'email'], tokenType: 'Bearer',
      }),
    };
    await updateAppStore(async (data) => {
      data.accounts = [{
        ...account,
        authMethod: 'app-password',
        proxy: { protocol: 'socks5', host: '127.0.0.1', port: 1080, username: 'proxy-user' },
        encryptedSecret: await encryptSecret({ authType: 'app-password', password: appPassword, proxyPassword }),
      }, oauthAccount];
      data.messages = [{
        id: 'export-must-not-include-message', accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 501,
        from: { name: 'Private Sender', address: 'sender@example.com' }, to: [{ name: 'Owner', address: account.email }],
        subject: 'must-not-export-subject', preview: 'must-not-export-preview', text: 'must-not-export-body',
        date: '2026-08-04T00:00:00.000Z', unread: true, flagged: false, hasAttachments: true,
        attachments: [{ filename: 'must-not-export-attachment.txt', contentType: 'text/plain', size: 7 }],
      }];
      data.drafts = [{
        id: '77777777-7777-4777-8777-777777777777', accountId: account.id, to: [], cc: [], subject: 'must-not-export-draft', text: 'must-not-export-draft-body', html: '', attachments: [],
        createdAt: '2026-08-04T00:00:00.000Z', updatedAt: '2026-08-04T00:00:00.000Z',
      }];
    });

    const unauthorized = await fetch(`${baseUrl}/api/security/mail-authorization-exports`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ currentPassword: 'test-password-123', exportPassword }),
    });
    expect(unauthorized.status).toBe(401);
    const wrongPassword = await request('/api/security/mail-authorization-exports', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ currentPassword: 'wrong-current-password', exportPassword }),
    });
    expect(wrongPassword.response.status).toBe(403);
    expect((await request('/api/security/mail-authorization-exports', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ currentPassword: 'test-password-123', exportPassword: 'too-short' }),
    })).response.status).toBe(400);

    const expiring = await request('/api/security/mail-authorization-exports', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ currentPassword: 'test-password-123', exportPassword }),
    });
    const expiringId = String(expiring.body.downloadPath).split('/').at(-1)!;
    const { consumeMailAuthorizationExport } = await import('./domain/privacy.js');
    expect(consumeMailAuthorizationExport(expiringId, appUserId, new Date(expiring.body.expiresAt).getTime() + 1)).toBeUndefined();
    expect((await fetch(`${baseUrl}${expiring.body.downloadPath}`, { headers: { Cookie: authCookie } })).status).toBe(404);

    const prepared = await request('/api/security/mail-authorization-exports', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ currentPassword: 'test-password-123', exportPassword }),
    });
    expect(prepared.response.status).toBe(200);
    expect(prepared.response.headers.get('cache-control')).toContain('no-store');
    expect(prepared.body).toMatchObject({
      downloadPath: expect.stringMatching(/^\/api\/security\/mail-authorization-exports\/[A-Za-z0-9_-]+$/),
      filename: expect.stringMatching(/^imail-mail-authorizations-\d{4}-\d{2}-\d{2}\.imailauth$/),
      accountCount: 2,
      expiresAt: expect.any(String),
    });
    expect(JSON.stringify(prepared.body)).not.toContain(appPassword);
    expect((await fetch(`${baseUrl}${prepared.body.downloadPath}`, { headers: { Cookie: privacyOtherCookie } })).status).toBe(404);

    const downloaded = await fetch(`${baseUrl}${prepared.body.downloadPath}`, { headers: { Cookie: authCookie } });
    expect(downloaded.status).toBe(200);
    expect(downloaded.headers.get('content-type')).toContain('application/vnd.imail.mail-authorization-export+json');
    expect(downloaded.headers.get('content-disposition')).toContain(prepared.body.filename);
    expect(downloaded.headers.get('cache-control')).toContain('no-store');
    const encryptedText = await downloaded.text();
    for (const secret of [appPassword, proxyPassword, accessToken, refreshToken, 'must-not-export-body', 'must-not-export-attachment.txt', 'must-not-export-draft-body']) expect(encryptedText).not.toContain(secret);
    const envelope = JSON.parse(encryptedText);
    expect(envelope).toMatchObject({
      format: 'imail-mail-authorizations', formatVersion: 1,
      kdf: { algorithm: 'scrypt', cost: 32768, blockSize: 8, parallelization: 1, keyLength: 32 },
      cipher: { algorithm: 'aes-256-gcm' }, ciphertext: expect.any(String),
    });
    const payload = await decryptAuthorizationExport(envelope, exportPassword);
    expect(payload).toMatchObject({ format: 'imail-mail-authorizations', formatVersion: 1, accounts: expect.any(Array) });
    expect(payload.accounts).toHaveLength(2);
    const passwordExport = payload.accounts.find((item: { email: string }) => item.email === account.email);
    expect(passwordExport).toMatchObject({
      provider: 'gmail', authMethod: 'app-password',
      settings: account.settings,
      proxy: { protocol: 'socks5', host: '127.0.0.1', port: 1080, username: 'proxy-user' },
      authorization: { authType: 'app-password', password: appPassword, proxyPassword },
    });
    const oauthExport = payload.accounts.find((item: { email: string }) => item.email === oauthAccount.email);
    expect(oauthExport.authorization).toMatchObject({ authType: 'oauth2', oauthProvider: 'google', accessToken, refreshToken, scopes: ['openid', 'email'], tokenType: 'Bearer' });
    for (const forbidden of ['id', 'ownerId', 'encryptedSecret', 'messages', 'drafts', 'contacts', 'tokens', 'mailboxes', 'lastSyncAt', 'lastError', 'status']) {
      expect(passwordExport).not.toHaveProperty(forbidden);
    }
    for (const excluded of ['must-not-export-body', 'must-not-export-attachment.txt', 'must-not-export-draft-body', 'Private Sender', 'test-password-123']) {
      expect(JSON.stringify(payload)).not.toContain(excluded);
    }
    expect((await fetch(`${baseUrl}${prepared.body.downloadPath}`, { headers: { Cookie: authCookie } })).status).toBe(404);
    const audit = await request('/api/security/audit-events?limit=20');
    expect(audit.body.events).toEqual(expect.arrayContaining([
      expect.objectContaining({ eventType: 'privacy.mail-authorization-export.prepared', detail: { accountCount: '2' } }),
      expect.objectContaining({ eventType: 'privacy.mail-authorization-export.downloaded', detail: { accountCount: '2' } }),
      expect.objectContaining({ eventType: 'sensitive-action.reauthentication-failed', detail: { action: 'mail-authorization-export' } }),
    ]));
    for (const secret of [appPassword, proxyPassword, accessToken, refreshToken, exportPassword]) expect(JSON.stringify(audit.body)).not.toContain(secret);
  });

  it('clears only the current user mailbox data after password reauthentication and exact confirmation', async () => {
    const { encryptSecret } = await import('./crypto.js');
    const otherAccount: MailAccount = {
      ...account,
      id: '44444444-4444-4444-8444-444444444444',
      email: 'privacy-other@example.com',
      encryptedSecret: await encryptSecret({ authType: 'app-password', password: 'other-mail-password' }),
    };
    await withUserContext(privacyOtherUserId, () => updateStore((data) => {
      data.accounts = [otherAccount]; data.messages = []; data.tokens = []; data.drafts = [];
    }));
    await updateAppStore(async (data) => {
      data.accounts = [{ ...account, authMethod: 'app-password', encryptedSecret: await encryptSecret({ authType: 'app-password', password: 'owner-mail-password' }) }];
      data.messages = [{
        id: 'clear-owner-message', accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 601,
        from: { name: 'Sender', address: 'sender@example.com' }, to: [], subject: 'Clear', preview: '', text: 'private body',
        date: '2026-08-04T00:00:00.000Z', unread: false, flagged: false, hasAttachments: false, attachments: [],
      }];
      data.drafts = [{ id: '55555555-5555-4555-8555-555555555555', accountId: account.id, to: [], cc: [], subject: 'Private draft', text: '', html: '', attachments: [], createdAt: '2026-08-04T00:00:00.000Z', updatedAt: '2026-08-04T00:00:00.000Z' }];
      data.tokens = [{ id: '66666666-6666-4666-8666-666666666666', name: 'Clear token', tokenHash: 'clear-token-hash', prefix: 'imail_clear', scopes: ['accounts:read'], accountIds: [account.id], createdAt: '2026-08-04T00:00:00.000Z', expiresAt: '2026-08-05T00:00:00.000Z' }];
    });
    const pending = await request('/api/security/mail-authorization-exports', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ currentPassword: 'test-password-123', exportPassword: 'pending-export-password' }),
    });
    expect(pending.response.status).toBe(200);

    const wrongConfirmation = await request('/api/security/clear-user-data', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ currentPassword: 'test-password-123', confirmation: '清除全部数据' }),
    });
    expect(wrongConfirmation.response.status).toBe(400);
    const wrongPassword = await request('/api/security/clear-user-data', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ currentPassword: 'wrong-current-password', confirmation: '清除我的邮箱数据' }),
    });
    expect(wrongPassword.response.status).toBe(403);
    expect((await request('/api/accounts')).body.accounts).toHaveLength(1);

    const cleared = await request('/api/security/clear-user-data', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ currentPassword: 'test-password-123', confirmation: '清除我的邮箱数据' }),
    });
    expect(cleared.response.status).toBe(204);
    expect((await request('/api/auth/session')).body.user.id).toBe(appUserId);
    expect((await request('/api/accounts')).body.accounts).toEqual([]);
    expect((await request('/api/messages')).body.messages).toEqual([]);
    expect((await request('/api/drafts')).body.drafts).toEqual([]);
    expect((await request('/api/developer-tokens')).body.tokens).toEqual([]);
    expect(getSyncStore().getPolicy(account.id)).toBeUndefined();
    expect((await fetch(`${baseUrl}${pending.body.downloadPath}`, { headers: { Cookie: authCookie } })).status).toBe(404);
    const otherAccounts = await fetch(`${baseUrl}/api/accounts`, { headers: { Cookie: privacyOtherCookie } }).then((response) => response.json());
    expect(otherAccounts.accounts).toEqual([expect.objectContaining({ id: otherAccount.id, email: otherAccount.email })]);
    const audit = await request('/api/security/audit-events?limit=20');
    expect(audit.body.events).toEqual(expect.arrayContaining([
      expect.objectContaining({ eventType: 'privacy.user-data-cleared', detail: { accountCount: '1' } }),
      expect.objectContaining({ eventType: 'sensitive-action.reauthentication-failed', detail: { action: 'clear-user-data' } }),
    ]));
    expect(JSON.stringify(audit.body)).not.toContain('owner-mail-password');
  });

  it('audits sensitive management actions without exposing authorization codes', async () => {
    const created = await request('/api/developer-tokens', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'Audited MCP agent', scopes: ['mcp:full'], mailboxes: [], ttlSeconds: 3600 }),
    });
    expect(created.response.status).toBe(201);
    const rawToken = String(created.body.token);
    const tokenId = String(created.body.detail.id);

    const afterCreate = await request('/api/security/audit-events?limit=20');
    expect(afterCreate.response.status).toBe(200);
    expect(afterCreate.body.events).toEqual(expect.arrayContaining([
      expect.objectContaining({ eventType: 'developer-token.created', detail: { tokenId, scopes: 'mcp:full', mailboxCount: '1' } }),
    ]));
    expect(JSON.stringify(afterCreate.body)).not.toContain(rawToken);
    expect(afterCreate.body.events.every((event: { actorHash: string }) => /^[0-9a-f]{64}$/.test(event.actorHash))).toBe(true);

    expect((await request(`/api/developer-tokens/${tokenId}`, { method: 'DELETE' })).response.status).toBe(204);
    const afterRevoke = await request('/api/security/audit-events?limit=20');
    expect(afterRevoke.body.events).toEqual(expect.arrayContaining([
      expect.objectContaining({ eventType: 'developer-token.revoked', detail: { tokenId } }),
    ]));
    expect((await request(`/api/accounts/${account.id}`, { method: 'DELETE' })).response.status).toBe(204);
    const afterAccountRemoval = await request('/api/security/audit-events?limit=20');
    expect(afterAccountRemoval.body.events).toEqual(expect.arrayContaining([
      expect.objectContaining({ eventType: 'account.removed', detail: { accountId: account.id } }),
    ]));
    expect(JSON.stringify(afterAccountRemoval.body)).not.toContain(account.encryptedSecret);
    expect((await fetch(`${baseUrl}/api/security/audit-events`)).status).toBe(401);
  });

  it('serves a full mail-management MCP endpoint only to mcp:full authorization codes', async () => {
    const ordinary = await request('/api/developer-tokens', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'Read only', scopes: ['messages:read'], mailboxes: [account.email], ttlSeconds: 3600 }),
    });
    const initialize = { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'vitest', version: '1.0.0' } } };
    const denied = await request('/mcp', { method: 'POST', headers: { 'Content-Type': 'application/json', Accept: 'application/json, text/event-stream', Authorization: `Bearer ${ordinary.body.token}` }, body: JSON.stringify(initialize) });
    expect(denied.response.status).toBe(401);

    const created = await request('/api/developer-tokens', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'MCP agent', scopes: ['messages:read', 'mcp:full'], mailboxes: [], ttlSeconds: 3600 }),
    });
    expect(created.body.token).toMatch(/^imail_mcp_/);
    expect(created.body.detail.scopes).toEqual(['mcp:full']);
    const headers = { 'Content-Type': 'application/json', Accept: 'application/json, text/event-stream', Authorization: `Bearer ${created.body.token}` };
    const mcp = async (payload: unknown) => {
      const response = await fetch(`${baseUrl}/mcp`, { method: 'POST', headers, body: JSON.stringify(payload) });
      const text = await response.text();
      const data = response.headers.get('content-type')?.includes('text/event-stream')
        ? text.split('\n').find((line) => line.startsWith('data: '))?.slice(6) ?? '{}'
        : text;
      return { response, body: JSON.parse(data) };
    };
    const initialized = await mcp(initialize);
    expect(initialized.response.status).toBe(200);
    expect(initialized.body.result.serverInfo).toMatchObject({ name: 'imail', version: '1.0.0' });

    const listed = await mcp({ jsonrpc: '2.0', id: 2, method: 'tools/list', params: {} });
    const toolNames = listed.body.result.tools.map((tool: { name: string }) => tool.name);
    expect(toolNames).toEqual(expect.arrayContaining([
      'settings_get', 'settings_update', 'theme_custom_get', 'theme_custom_update', 'accounts_list', 'account_add_with_code', 'account_start_oauth', 'account_update_authorization_code', 'account_proxy_update', 'account_remove',
      'mailbox_sync', 'sync_policy_get', 'sync_policy_update', 'messages_list', 'message_get', 'message_update', 'message_move', 'message_send', 'attachment_download',
      'drafts_list', 'draft_get', 'draft_save', 'draft_delete', 'labels_list', 'notifications_list',
    ]));

    const auditedSettingsUpdate = await mcp({ jsonrpc: '2.0', id: 3, method: 'tools/call', params: { name: 'settings_update', arguments: { startupView: 'inbox' } } });
    expect(auditedSettingsUpdate.body.result.isError).not.toBe(true);
    const audit = await request('/api/security/audit-events?limit=50');
    expect(audit.body.events).toEqual(expect.arrayContaining([
      expect.objectContaining({
        eventType: 'mcp.management-tool-called',
        detail: { tool: 'settings_update', authorizationCodeId: created.body.detail.id },
      }),
    ]));
    expect(JSON.stringify(audit.body)).not.toContain(created.body.token);
    const called = await mcp({ jsonrpc: '2.0', id: 3, method: 'tools/call', params: { name: 'accounts_list', arguments: {} } });
    expect(called.body.result.structuredContent.accounts[0]).toMatchObject({ email: account.email, displayName: account.displayName });
    expect(JSON.stringify(called.body)).not.toContain(account.encryptedSecret);
    const policyUpdated = await mcp({ jsonrpc: '2.0', id: 31, method: 'tools/call', params: { name: 'sync_policy_update', arguments: { email: account.email, folderMode: 'standard', notifyOnError: false } } });
    expect(policyUpdated.body.result.structuredContent.policy).toMatchObject({ folderMode: 'standard', notifyOnError: false });
    const policyRead = await mcp({ jsonrpc: '2.0', id: 32, method: 'tools/call', params: { name: 'sync_policy_get', arguments: { email: account.email } } });
    expect(policyRead.body.result.structuredContent.accounts[0]).toMatchObject({ accountEmail: account.email, policy: { folderMode: 'standard', notifyOnError: false } });
    expect(JSON.stringify(policyRead.body)).not.toContain(account.encryptedSecret);
    const settingsUpdated = await mcp({ jsonrpc: '2.0', id: 33, method: 'tools/call', params: { name: 'settings_update', arguments: { theme: 'soft-neubrutalism', defaultMessageView: 'rendered', notificationKinds: { unread: false } } } });
    expect(settingsUpdated.body.result.structuredContent.preferences).toMatchObject({ theme: 'soft-neubrutalism', defaultMessageView: 'rendered', notificationKinds: { unread: false, snooze: true, error: true }, shortcutBindings: { focusSearch: 'Mod+K' } });
    const settingsRead = await mcp({ jsonrpc: '2.0', id: 34, method: 'tools/call', params: { name: 'settings_get', arguments: {} } });
    expect(settingsRead.body.result.structuredContent.preferences).toEqual(settingsUpdated.body.result.structuredContent.preferences);
    const customTheme = {
      name: 'Agent Ocean', canvas: '#edf3f7', surface: '#ffffff', surfaceSubtle: '#f2f7fa', rail: '#13293d', text: '#17212b',
      textSecondary: '#5d6b78', border: '#cad7e0', accent: '#168aad', accentSubtle: '#dff3f8', radius: 'rounded', shadow: 'soft', typography: 'technical',
    };
    const themeUpdated = await mcp({ jsonrpc: '2.0', id: 35, method: 'tools/call', params: { name: 'theme_custom_update', arguments: customTheme } });
    expect(themeUpdated.body.result.structuredContent.theme).toEqual(customTheme);
    const themeRead = await mcp({ jsonrpc: '2.0', id: 36, method: 'tools/call', params: { name: 'theme_custom_get', arguments: {} } });
    expect(themeRead.body.result.structuredContent.theme).toEqual(customTheme);
    const gatewayPreferences = await request('/api/preferences');
    expect(gatewayPreferences.body.preferences).not.toHaveProperty('customTheme');

    await updateAppStore((data) => { data.messages = [{
      id: 'mcp-message', accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 42,
      from: { name: 'Agent Sender', address: 'sender@example.com' }, to: [{ name: 'Owner', address: account.email }],
      subject: 'MCP test', preview: 'Cached preview', text: 'Cached body', html: '<p>Cached body</p>', date: '2026-07-29T12:00:00.000Z',
      unread: true, flagged: false, hasAttachments: false, attachments: [], labels: [],
    }]; });
    const messages = await mcp({ jsonrpc: '2.0', id: 4, method: 'tools/call', params: { name: 'messages_list', arguments: { email: account.email } } });
    expect(messages.body.result.structuredContent).toMatchObject({ total: 1, messages: [{ id: 'mcp-message', accountEmail: account.email }] });
    const detail = await mcp({ jsonrpc: '2.0', id: 5, method: 'tools/call', params: { name: 'message_get', arguments: { messageId: 'mcp-message' } } });
    expect(detail.body.result.structuredContent.message).toMatchObject({ text: 'Cached body', html: '<p>Cached body</p>' });
    const labeled = await mcp({ jsonrpc: '2.0', id: 6, method: 'tools/call', params: { name: 'message_update', arguments: { messageId: 'mcp-message', labels: ['MCP'] } } });
    expect(labeled.body.result.structuredContent.message.labels).toEqual(['MCP']);

    const saved = await mcp({ jsonrpc: '2.0', id: 7, method: 'tools/call', params: { name: 'draft_save', arguments: { accountEmail: account.email, to: ['friend@example.com'], subject: 'Agent draft', text: 'Draft body' } } });
    const draftId = saved.body.result.structuredContent.draft.id;
    expect(draftId).toMatch(/[0-9a-f-]{36}/);
    const drafts = await mcp({ jsonrpc: '2.0', id: 8, method: 'tools/call', params: { name: 'drafts_list', arguments: {} } });
    expect(drafts.body.result.structuredContent.drafts[0]).toMatchObject({ id: draftId, subject: 'Agent draft', accountEmail: account.email });
    const deleted = await mcp({ jsonrpc: '2.0', id: 9, method: 'tools/call', params: { name: 'draft_delete', arguments: { draftId } } });
    expect(deleted.body.result.structuredContent).toEqual({ deleted: true, draftId });
  });

  it('reports health and OAuth provider capability metadata', async () => {
    const health = await request('/api/health');
    expect(health.response.status).toBe(200); expect(health.body).toEqual({ ok: true, service: 'imail' });
    const providers = await request('/api/providers');
    expect(providers.body.providers.map((item: { id: string }) => item.id)).toEqual(['outlook', 'gmail', 'qq', 'yahoo', 'hotmail', 'icloud', 'custom']);
    expect(providers.body.oauth).toHaveLength(3);
    expect(providers.body.providers.find((item: { id: string }) => item.id === 'qq')).toMatchObject({ authMode: 'authorization-code', oauthProvider: null });
    expect(providers.body.providers.find((item: { id: string }) => item.id === 'outlook')).toMatchObject({ authMode: 'oauth2', oauthProvider: 'microsoft', fallbackAuthMode: 'app-password', helpUrl: expect.stringMatching(/^https:\/\//) });
    expect(providers.body.providers.find((item: { id: string }) => item.id === 'hotmail')).toMatchObject({ authMode: 'oauth2', oauthTenant: 'consumers', fallbackAuthMode: 'app-password', helpUrl: expect.stringMatching(/^https:\/\//) });
  });

  it('never exposes encrypted mailbox credentials', async () => {
    const result = await request('/api/accounts');
    expect(result.response.status).toBe(200);
    expect(result.body.accounts[0]).toMatchObject({ id: account.id, email: account.email, authMethod: 'oauth2' });
    expect(JSON.stringify(result.body)).not.toContain('must-never-leak');
    expect(result.body.accounts[0]).not.toHaveProperty('encryptedSecret');
  });

  it('returns a stable conflict for a duplicate mailbox account', async () => {
    const result = await request('/api/accounts', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        provider: account.provider, email: account.email, displayName: account.displayName, group: account.group,
        color: account.color, password: 'duplicate-password', settings: account.settings,
      }),
    });
    expect(result.response.status).toBe(409);
    expect(result.body).toEqual({ error: '这个邮箱已经添加' });
  });

  it('persists push-first automatic sync settings and queues work without executing IMAP in the request', async () => {
    const defaults = await request('/api/sync-policy');
    expect(defaults.body.policy).toEqual({ enabled: true, folderMode: 'inbox', selectedMailboxes: [], notifyOnError: true });
    const updated = await request(`/api/accounts/${account.id}/sync-policy`, {
      method: 'PATCH', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ folderMode: 'standard', notifyOnError: false }),
    });
    expect(updated.body.policy).toMatchObject({ accountId: account.id, folderMode: 'standard', notifyOnError: false });
    expect(updated.body.policy).not.toHaveProperty('intervalMinutes');
    const queued = await request(`/api/accounts/${account.id}/sync`, { method: 'POST' });
    expect(queued.body).toMatchObject({ queued: true, synced: 0, jobId: expect.any(String) });
    const duplicate = await request(`/api/accounts/${account.id}/mailboxes/sync`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ mailbox: 'INBOX' }),
    });
    expect(duplicate.body.jobId).toBe(queued.body.jobId);
    const status = await request('/api/sync-status');
    expect(status.body.accounts[0]).toMatchObject({ accountId: account.id, policy: { folderMode: 'standard', notifyOnError: false }, jobs: [{ id: queued.body.jobId, status: 'queued', reason: 'manual' }] });
    expect(status.body.accounts[0].jobs).toHaveLength(1);
    expect(JSON.stringify(status.body)).not.toContain(account.encryptedSecret);

    const eventResponse = await fetch(`${baseUrl}/api/events`, { headers: { Cookie: authCookie } });
    expect(eventResponse.status).toBe(200);
    const reader = eventResponse.body!.getReader();
    const decoder = new TextDecoder();
    let eventText = '';
    while (!eventText.includes('event: sync.status')) {
      const chunk = await reader.read();
      if (chunk.done) break;
      eventText += decoder.decode(chunk.value, { stream: true });
    }
    await reader.cancel();
    const statusEvent = eventText.match(/event: sync\.status\ndata: ([^\n]+)/);
    expect(statusEvent).not.toBeNull();
    expect(JSON.parse(statusEvent![1]).accounts[0]).toMatchObject({
      accountId: account.id,
      policy: { folderMode: 'standard', notifyOnError: false },
      jobs: [{ id: queued.body.jobId, status: 'queued' }],
    });
    expect(statusEvent![1]).not.toContain(account.encryptedSecret);
  });

  it('updates account workspace metadata without exposing credentials', async () => {
    const result = await request(`/api/accounts/${account.id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ group: '客户支持', groupIcon: 'users', displayName: 'Support' }) });
    expect(result.response.status).toBe(200);
    expect(result.body.account).toMatchObject({ group: '客户支持', groupIcon: 'users', displayName: 'Support' });
    expect(result.body.account).not.toHaveProperty('encryptedSecret');
  });

  it('deletes an account together with its drafts and sync data', async () => {
    const created = await request('/api/drafts', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ accountId: account.id, to: [], cc: [], subject: 'Temporary', text: '', html: '', attachments: [] }),
    });
    expect(created.response.status).toBe(201);
    const removed = await request(`/api/accounts/${account.id}`, { method: 'DELETE' });
    expect(removed.response.status).toBe(204);
    expect((await request('/api/accounts')).body.accounts).toEqual([]);
    expect((await request('/api/drafts')).body.drafts).toEqual([]);
    expect(getSyncStore().getPolicy(account.id)).toBeUndefined();
  });

  it('cannot move a draft to another application user mailbox or delete its sync data', async () => {
    const foreignAccount = { ...account, id: '22222222-2222-4222-8222-222222222222', email: 'foreign@example.com' };
    await withUserContext('foreign-user', () => updateStore((data) => { data.accounts = [foreignAccount]; data.messages = []; data.tokens = []; data.drafts = []; }));
    getSyncStore().ensurePolicy(foreignAccount.id);
    try {
      const created = await request('/api/drafts', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ accountId: account.id, to: [], cc: [], subject: 'Owned', text: '', html: '', attachments: [] }),
      });
      const moved = await request(`/api/drafts/${created.body.draft.id}`, {
        method: 'PUT', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ accountId: foreignAccount.id, to: [], cc: [], subject: 'Injected', text: '', html: '', attachments: [] }),
      });
      expect(moved.response.status).toBe(404);
      expect(moved.body.error).toBe('发件邮箱不存在');
      const removed = await request(`/api/accounts/${foreignAccount.id}`, { method: 'DELETE' });
      expect(removed.response.status).toBe(404);
      expect(getSyncStore().getPolicy(foreignAccount.id)).toBeDefined();
    } finally {
      getSyncStore().deleteAccountData(foreignAccount.id);
      await withUserContext('foreign-user', () => updateStore((data) => { data.accounts = []; data.messages = []; data.tokens = []; data.drafts = []; }));
    }
  });

  it('returns a stable not-found response instead of an internal storage error for a missing draft', async () => {
    const result = await request('/api/drafts/00000000-0000-4000-8000-000000000099', {
      method: 'PUT', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ accountId: account.id, to: [], cc: [], subject: 'Missing', text: '', html: '', attachments: [] }),
    });
    expect(result.response.status).toBe(404);
    expect(result.body).toEqual({ error: '草稿不存在' });
  });

  it('issues a scoped developer token and authorizes its permitted API', async () => {
    const created = await request('/api/developer-tokens', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'API test', scopes: ['accounts:read'], mailboxes: [account.email], ttlSeconds: 3600 }),
    });
    expect(created.response.status).toBe(201); expect(created.body.token).toMatch(/^imail_/);
    const allowed = await request('/gateway/v1/mailboxes', { headers: { Authorization: `Bearer ${created.body.token}` } });
    expect(allowed.response.status).toBe(200); expect(allowed.body.mailboxes).toHaveLength(1);
    expect(allowed.body.mailboxes[0]).toMatchObject({ email: account.email, displayName: account.displayName });
    expect(allowed.body.mailboxes[0]).not.toHaveProperty('id');
    expect(allowed.body.mailboxes[0]).not.toHaveProperty('settings');
    const denied = await request('/gateway/v1/messages', { headers: { Authorization: `Bearer ${created.body.token}` } });
    expect(denied.response.status).toBe(401);
    expect(denied.body.error).toMatchObject({ code: 'UNAUTHORIZED', message: expect.any(String), requestId: expect.any(String) });
    expect(denied.response.headers.get('x-request-id')).toBe(denied.body.error.requestId);
    const listing = await request('/api/developer-tokens');
    expect(JSON.stringify(listing.body)).not.toContain(created.body.token);
    expect(listing.body.tokens[0]).not.toHaveProperty('tokenHash');
  });

  it('validates malformed requests and unknown reconnect targets', async () => {
    const invalid = await request('/api/developer-tokens', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
    expect(invalid.response.status).toBe(400);
    const missing = await request('/api/accounts/missing/oauth/reconnect', { method: 'POST' });
    expect(missing.response.status).toBe(404); expect(missing.body.error).toBe('邮箱账户不存在');
    const missingTest = await request('/api/accounts/missing/connection-test', { method: 'POST' });
    expect(missingTest.response.status).toBe(404); expect(missingTest.body.error).toBe('邮箱账户不存在');
  });

  it('retries a connection with saved authorization without exposing or replacing credentials', async () => {
    const checked = await request(`/api/accounts/${account.id}/connection-test`, { method: 'POST' });
    expect(checked.response.status).toBe(200);
    expect(checked.body.account).toMatchObject({ id: account.id, authMethod: 'oauth2', status: 'error' });
    expect(checked.body.account).not.toHaveProperty('encryptedSecret');
    const stored = await request('/api/accounts');
    expect(stored.body.accounts[0].status).toBe('error');
  });

  it('does not allow replacing an OAuth token through the app-password credential endpoint', async () => {
    const result = await request(`/api/accounts/${account.id}/credential`, {
      method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password: 'must-not-replace-oauth' }),
    });
    expect(result.response.status).toBe(409);
    expect(result.body.error).toBe('OAuth 邮箱请使用重新授权');
  });

  it('returns paged summaries and loads a single message body on demand', async () => {
    await updateAppStore((data) => { data.messages = [{
      id: 'message-lazy', accountId: account.id, mailbox: 'INBOX', uid: 8, from: { name: 'Sender', address: 'sender@example.com' }, to: [],
      subject: 'Lazy body', preview: 'Preview', text: 'Full body', html: '<p>Full body</p>', date: '2026-07-28T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [],
    }]; });
    const page = await request('/api/messages?limit=1&offset=0');
    expect(page.body).toMatchObject({ total: 1, nextOffset: 1, hasMore: false });
    expect(page.body.messages[0].from.logo.url).toBe('/api/contacts/logo?address=sender%40example.com');
    expect(page.body.messages[0]).not.toHaveProperty('text'); expect(page.body.messages[0]).not.toHaveProperty('html');
    const detail = await request('/api/messages/message-lazy');
    expect(detail.body.message).toMatchObject({ text: 'Full body', html: '<p>Full body</p>' });
    expect(detail.body.message.from.logo.url).toBe('/api/contacts/logo?address=sender%40example.com');
    expect((await request('/api/messages/missing')).response.status).toBe(404);
    expect((await request('/api/message-stats')).body).toMatchObject({ total: 1, unread: 1, byAccount: [{ accountId: account.id, total: 1, unread: 1 }] });
    const markedRead = await request('/api/messages/message-lazy', {
      method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ unread: false }),
    });
    expect(markedRead.response.status).toBe(200);
    expect(markedRead.body.message).toMatchObject({ id: 'message-lazy', unread: false });
    expect(markedRead.body.message.from.logo.url).toBe('/api/contacts/logo?address=sender%40example.com');
    expect((await request('/api/message-stats')).body).toMatchObject({ total: 1, unread: 0, byAccount: [{ accountId: account.id, total: 1, unread: 0 }] });
    expect((await request('/api/messages?unread=true&limit=10&offset=0')).body).toMatchObject({ total: 0, messages: [] });
    const archived = await request('/api/messages/message-lazy/move', {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ destination: 'archive' }),
    });
    expect(archived.response.status).toBe(200);
    expect(archived.body).toMatchObject({ destination: 'archive', mailbox: 'Archive', message: { id: 'message-lazy' } });
    expect(archived.body.message.from.logo.url).toBe('/api/contacts/logo?address=sender%40example.com');
    expect((await request('/api/messages/message-lazy')).body.message).toMatchObject({ mailbox: 'Archive', mailboxRole: 'archive' });
    expect((await request('/api/messages?mailboxRole=archive')).body).toMatchObject({ total: 1, messages: [{ id: 'message-lazy', mailboxRole: 'archive' }] });
    expect((await request('/api/message-stats')).body).toMatchObject({ total: 0, unread: 0 });
  });

  it('builds a contact library from every cached sender and recipient', async () => {
    await updateAppStore((data) => { data.messages = [
      { id: 'received-1', accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 12, from: { name: 'Alice', address: 'Alice@example.com' }, to: [{ name: 'Owner', address: account.email }], subject: 'Received', preview: '', text: '', date: '2026-07-29T02:00:00.000Z', unread: false, flagged: false, hasAttachments: false, attachments: [] },
      { id: 'sent-1', accountId: account.id, mailbox: 'Sent', mailboxRole: 'sent', uid: 13, from: { name: 'Owner', address: account.email }, to: [{ name: 'Alice Zhang', address: 'alice@example.com' }, { name: 'Bob', address: 'bob@example.com' }], subject: 'Sent', preview: '', text: '', date: '2026-07-29T03:00:00.000Z', unread: false, flagged: false, hasAttachments: false, attachments: [] },
    ]; });

    const result = await request('/api/contacts');
    expect(result.response.status).toBe(200);
    expect(result.body.contacts).toEqual([
      { address: 'alice@example.com', name: 'Alice Zhang', messageCount: 2, lastContactAt: '2026-07-29T03:00:00.000Z', logo: { url: '/api/contacts/logo?address=alice%40example.com' } },
      { address: 'bob@example.com', name: 'Bob', messageCount: 1, lastContactAt: '2026-07-29T03:00:00.000Z', logo: { url: '/api/contacts/logo?address=bob%40example.com' } },
    ]);
    expect(JSON.stringify(result.body)).not.toContain(account.email);
  });

  it('persists drafts and exposes labels, snooze state and notifications', async () => {
    const partialDraft = await request('/api/drafts', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ accountId: account.id, to: ['@'], subject: '', text: '' }) });
    expect(partialDraft.response.status).toBe(201);
    expect(partialDraft.body.draft.to).toEqual(['@']);
    expect((await request(`/api/drafts/${partialDraft.body.draft.id}`, { method: 'DELETE' })).response.status).toBe(204);
    const createdDraft = await request('/api/drafts', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ accountId: account.id, to: ['friend@example.com'], subject: 'Draft subject', text: 'Draft body', html: '<p><strong>Draft body</strong></p>', attachments: [{ id: 'attachment-1', filename: 'note.txt', contentType: 'text/plain', size: 5, data: 'aGVsbG8=' }] }) });
    expect(createdDraft.response.status).toBe(201);
    expect((await request('/api/drafts')).body.drafts[0]).toMatchObject({ subject: 'Draft subject', to: ['friend@example.com'], html: '<p><strong>Draft body</strong></p>', attachments: [{ filename: 'note.txt', size: 5 }] });
    const draftId = createdDraft.body.draft.id;
    const updatedDraft = await request(`/api/drafts/${draftId}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ accountId: account.id, to: [], cc: [], subject: 'Updated draft', text: '' }) });
    expect(updatedDraft.body.draft.subject).toBe('Updated draft');

    await updateAppStore((data) => { data.messages = [{ id: 'organize-me', accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 10, from: { name: 'Sender', address: 'sender@example.com' }, to: [], subject: 'Organize', preview: '', text: 'Body', date: '2026-07-29T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [], labels: [] }]; });
    const organized = await request('/api/messages/organize-me', { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ labels: ['客户'], snoozedUntil: '2999-01-01T09:00:00.000Z' }) });
    expect(organized.body.message).toMatchObject({ labels: ['客户'], snoozedUntil: '2999-01-01T09:00:00.000Z' });
    expect(organized.body.message.from.logo.url).toBe('/api/contacts/logo?address=sender%40example.com');
    expect((await request('/api/labels')).body.labels).toEqual(['客户']);
    expect((await request('/api/messages?mailboxRole=inbox')).body.total).toBe(0);
    expect((await request('/api/messages?mailboxRole=inbox&snoozed=true')).body.messages[0].id).toBe('organize-me');
    const notifications = await request('/api/notifications');
    expect(notifications.response.status).toBe(200);
    expect(Array.isArray(notifications.body.notifications)).toBe(true);
    expect((await request(`/api/drafts/${draftId}`, { method: 'DELETE' })).response.status).toBe(204);
    expect((await request('/api/drafts')).body.drafts).toEqual([]);
  });

  it('creates a draft idempotently when the client repeats its stable draft id', async () => {
    const draftId = '22222222-2222-4222-8222-222222222222';
    const headers = { 'Content-Type': 'application/json', 'X-Draft-Id': draftId };
    const first = await request('/api/drafts', { method: 'POST', headers, body: JSON.stringify({ accountId: account.id, to: [], cc: [], subject: 'First', text: '' }) });
    const repeated = await request('/api/drafts', { method: 'POST', headers, body: JSON.stringify({ accountId: account.id, to: [], cc: [], subject: 'Latest', text: '' }) });

    expect(first.response.status).toBe(201);
    expect(repeated.response.status).toBe(201);
    expect(first.body.draft.id).toBe(draftId);
    expect(repeated.body.draft).toMatchObject({ id: draftId, subject: 'Latest', createdAt: first.body.draft.createdAt });
    expect((await request('/api/drafts')).body.drafts).toHaveLength(1);
  });

  it('streams attachment downloads with safe response headers', async () => {
    const response = await fetch(`${baseUrl}/api/messages/message-file/attachments/0`, { headers: { Cookie: authCookie } });
    expect(response.status).toBe(200);
    expect(response.headers.get('content-type')).toContain('text/plain');
    expect(response.headers.get('content-disposition')).toContain("filename*=UTF-8''report.txt");
    expect(await response.text()).toBe('hello');
  });

  it('lets a developer token select a mailbox by email without exposing internal account IDs', async () => {
    await updateAppStore((data) => { data.messages = [
      { id: 'message-new', accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 11, from: { name: 'Alice', address: 'alice@example.com' }, to: [], subject: 'Newest gateway message', preview: 'Newest preview', text: 'Newest body', date: '2026-07-29T02:00:00.000Z', unread: true, flagged: false, hasAttachments: true, attachments: [{ filename: 'report.txt', contentType: 'text/plain', size: 5, index: 0 }], labels: [] },
      { id: 'message-dev', accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 9, from: { name: 'Sender', address: 'sender@example.com' }, to: [], subject: 'Gateway message', preview: 'Preview', text: 'Body', date: '2026-07-28T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [], labels: [] },
      { id: 'message-old', accountId: account.id, mailbox: 'Sent', mailboxRole: 'sent', uid: 7, from: { name: 'Owner', address: account.email }, to: [], subject: 'Old sent message', preview: 'Sent', text: 'Sent body', date: '2026-07-27T00:00:00.000Z', unread: false, flagged: false, hasAttachments: false, attachments: [], labels: [] },
    ]; });
    const created = await request('/api/developer-tokens', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'Gateway', scopes: ['messages:read', 'messages:send'], mailboxes: [account.email], ttlSeconds: 3600 }),
    });
    const headers = { Authorization: `Bearer ${created.body.token}` };
    const byRoute = await request(`/gateway/v1/mailboxes/${encodeURIComponent(account.email)}/messages?limit=10`, { headers });
    const firstPage = await request(`/gateway/v1/messages?mailbox=${encodeURIComponent(account.email)}&mailboxRole=inbox&limit=1`, { headers });
    expect(byRoute.body.page).toMatchObject({ count: 3, hasMore: false, nextCursor: null });
    expect(firstPage.body.page).toMatchObject({ limit: 1, count: 1, hasMore: true, nextCursor: expect.any(String) });
    expect(firstPage.body.messages[0]).toMatchObject({ id: 'message-new', accountEmail: account.email, folder: 'INBOX' });
    expect(firstPage.body.messages[0]).not.toHaveProperty('text');
    const secondPage = await request(`/gateway/v1/messages?mailbox=${encodeURIComponent(account.email)}&mailboxRole=inbox&limit=1&cursor=${encodeURIComponent(firstPage.body.page.nextCursor)}`, { headers });
    expect(secondPage.body.messages[0].id).toBe('message-dev');
    const detail = await request('/gateway/v1/messages/message-dev', { headers });
    expect(detail.body.message).toMatchObject({ id: 'message-dev', text: 'Body', accountEmail: account.email });
    expect(detail.body.message).not.toHaveProperty('accountId');
    const attachment = await fetch(`${baseUrl}/gateway/v1/messages/message-new/attachments/0`, { headers });
    expect(attachment.status).toBe(200); expect(await attachment.text()).toBe('hello');
    const byEmail = firstPage;
    expect(byEmail.body.messages[0]).not.toHaveProperty('accountId');
    expect(byEmail.body.messages[0]).not.toHaveProperty('uid');
    const missing = await request('/gateway/v1/messages?mailbox=missing@example.com', { headers });
    expect(missing.response.status).toBe(404); expect(missing.body.error.code).toBe('MAILBOX_NOT_AVAILABLE');
    const oldRoute = await request(`/gateway/v1/accounts/${account.id}/messages`, { headers });
    expect(oldRoute.response.status).toBe(404); expect(oldRoute.body.error.code).toBe('ENDPOINT_NOT_FOUND');
    const oldQuery = await request(`/gateway/v1/messages?accountId=${account.id}`, { headers });
    expect(oldQuery.response.status).toBe(400); expect(oldQuery.body.error.code).toBe('INVALID_REQUEST');
    const invalidCursor = await request('/gateway/v1/messages?cursor=broken', { headers });
    expect(invalidCursor.response.status).toBe(400); expect(invalidCursor.body.error.code).toBe('INVALID_CURSOR');
    const legacySend = await request('/gateway/v1/send', {
      method: 'POST', headers: { ...headers, 'Content-Type': 'application/json' },
      body: JSON.stringify({ accountId: account.id, to: ['recipient@example.com'], subject: 'Legacy', text: 'Not sent' }),
    });
    expect(legacySend.response.status).toBe(400);
    expect(legacySend.body.error.code).toBe('INVALID_REQUEST');
  });

  it('serves a UTF-8 lightweight interactive API console and OpenAPI contract', async () => {
    const specification = await request('/gateway/openapi.json');
    expect(specification.body.info).toMatchObject({ title: 'iMail Developer Gateway', version: '1.0.0' });
    expect(specification.body.paths).toHaveProperty('/messages/{messageId}');
    expect(specification.body.paths).toHaveProperty('/messages/{messageId}/attachments/{index}');
    expect(JSON.stringify(specification.body)).not.toContain('accountId');
    const page = await fetch(`${baseUrl}/gateway/docs`);
    expect(page.status).toBe(200);
    expect(page.headers.get('content-type')).toContain('text/html; charset=utf-8');
    const html = await page.text();
    expect(html).toContain('Lightweight API Console');
    expect(html).toContain('邮件能力');
    expect(html).toContain('连接并订阅');
    expect(html).toContain("spec['x-websocket']");
    expect(html.toLowerCase()).not.toContain('swagger');
    expect(html).not.toContain('<script src=');
    expect(page.headers.get('content-security-policy')).toContain("default-src 'self'");
    expect((await fetch(`${baseUrl}/api/dev/v1/health`, { headers: { Cookie: authCookie } })).status).toBe(404);
  });

  it('deletes account-owned cache and removes it from token grants', async () => {
    await request('/api/developer-tokens', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'Delete test', scopes: ['accounts:read'], mailboxes: [account.email], ttlSeconds: 3600 }),
    });
    await updateAppStore((data) => { data.messages.push({
      id: 'message-1', accountId: account.id, mailbox: 'INBOX', uid: 1, from: { name: '', address: 'sender@example.com' }, to: [],
      subject: 'Subject', preview: '', text: '', date: '2026-07-28T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [],
    }); });
    const removed = await request(`/api/accounts/${account.id}`, { method: 'DELETE' });
    expect(removed.response.status).toBe(204);
    expect((await request('/api/accounts')).body.accounts).toEqual([]);
    expect((await request('/api/messages')).body.messages).toEqual([]);
    expect((await request('/api/developer-tokens')).body.tokens[0].mailboxes).toEqual([]);
  });
});
