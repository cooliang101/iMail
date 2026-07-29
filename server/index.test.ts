import { mkdtemp, rm } from 'node:fs/promises';
import type { Server } from 'node:http';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MailAccount } from './types.js';

vi.mock('./mail.js', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./mail.js')>();
  return { ...actual, updateRemoteMessageFlags: vi.fn(async () => undefined) };
});

let directory: string;
let server: Server;
let baseUrl: string;
let updateStore: typeof import('./store.js')['updateStore'];
let closeStore: typeof import('./store.js')['closeStore'];

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
  const [{ app }, store] = await Promise.all([import('./index.js'), import('./store.js')]);
  updateStore = store.updateStore;
  closeStore = store.closeStore;
  server = app.listen(0, '127.0.0.1');
  await new Promise<void>((resolve) => server.once('listening', resolve));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('测试服务器启动失败');
  baseUrl = `http://127.0.0.1:${address.port}`;
});

afterAll(async () => {
  await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  closeStore();
  delete process.env.IMAIL_DATA_DIR; delete process.env.APP_MASTER_KEY;
  await rm(directory, { recursive: true, force: true });
});

beforeEach(async () => {
  await updateStore((data) => { data.accounts = [account]; data.messages = []; data.tokens = []; });
});

async function request(route: string, init?: RequestInit) {
  const response = await fetch(`${baseUrl}${route}`, init);
  const body = response.status === 204 ? undefined : await response.json();
  return { response, body };
}

describe('iMail HTTP API', () => {
  it('reports health and OAuth provider capability metadata', async () => {
    const health = await request('/api/health');
    expect(health.response.status).toBe(200); expect(health.body).toEqual({ ok: true, service: 'imail' });
    const providers = await request('/api/providers');
    expect(providers.body.providers.map((item: { id: string }) => item.id)).toEqual(['outlook', 'gmail', 'qq', 'yahoo', 'hotmail', 'icloud', 'custom']);
    expect(providers.body.oauth).toHaveLength(3);
    expect(providers.body.providers.find((item: { id: string }) => item.id === 'qq')).toMatchObject({ authMode: 'authorization-code', oauthProvider: null });
    expect(providers.body.providers.find((item: { id: string }) => item.id === 'hotmail')).toMatchObject({ authMode: 'oauth2', oauthTenant: 'consumers', fallbackAuthMode: null });
  });

  it('never exposes encrypted mailbox credentials', async () => {
    const result = await request('/api/accounts');
    expect(result.response.status).toBe(200);
    expect(result.body.accounts[0]).toMatchObject({ id: account.id, email: account.email, authMethod: 'oauth2' });
    expect(JSON.stringify(result.body)).not.toContain('must-never-leak');
    expect(result.body.accounts[0]).not.toHaveProperty('encryptedSecret');
  });

  it('issues a scoped developer token and authorizes its permitted API', async () => {
    const created = await request('/api/developer-tokens', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'API test', scopes: ['accounts:read'], accountIds: [account.id], ttlSeconds: 3600 }),
    });
    expect(created.response.status).toBe(201); expect(created.body.token).toMatch(/^imail_/);
    const allowed = await request('/api/dev/v1/accounts', { headers: { Authorization: `Bearer ${created.body.token}` } });
    expect(allowed.response.status).toBe(200); expect(allowed.body.accounts).toHaveLength(1);
    const denied = await request('/api/dev/v1/messages', { headers: { Authorization: `Bearer ${created.body.token}` } });
    expect(denied.response.status).toBe(401);
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
    await updateStore((data) => { data.messages = [{
      id: 'message-lazy', accountId: account.id, mailbox: 'INBOX', uid: 8, from: { name: 'Sender', address: 'sender@example.com' }, to: [],
      subject: 'Lazy body', preview: 'Preview', text: 'Full body', html: '<p>Full body</p>', date: '2026-07-28T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [],
    }]; });
    const page = await request('/api/messages?limit=1&offset=0');
    expect(page.body).toMatchObject({ total: 1, nextOffset: 1, hasMore: false });
    expect(page.body.messages[0]).not.toHaveProperty('text'); expect(page.body.messages[0]).not.toHaveProperty('html');
    const detail = await request('/api/messages/message-lazy');
    expect(detail.body.message).toMatchObject({ text: 'Full body', html: '<p>Full body</p>' });
    expect((await request('/api/messages/missing')).response.status).toBe(404);
    expect((await request('/api/message-stats')).body).toMatchObject({ total: 1, unread: 1, byAccount: [{ accountId: account.id, total: 1, unread: 1 }] });
    const markedRead = await request('/api/messages/message-lazy', {
      method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ unread: false }),
    });
    expect(markedRead.response.status).toBe(200);
    expect(markedRead.body.message).toMatchObject({ id: 'message-lazy', unread: false });
    expect((await request('/api/message-stats')).body).toMatchObject({ total: 1, unread: 0, byAccount: [{ accountId: account.id, total: 1, unread: 0 }] });
    expect((await request('/api/messages?unread=true&limit=10&offset=0')).body).toMatchObject({ total: 0, messages: [] });
  });

  it('lets a developer token select a mailbox by route, email or provider', async () => {
    await updateStore((data) => { data.messages = [{
      id: 'message-dev', accountId: account.id, mailbox: 'INBOX', uid: 9, from: { name: 'Sender', address: 'sender@example.com' }, to: [],
      subject: 'Gateway message', preview: '', text: 'Body', date: '2026-07-28T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [],
    }]; });
    const created = await request('/api/developer-tokens', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'Gateway', scopes: ['messages:read'], accountIds: [account.id], ttlSeconds: 3600 }),
    });
    const headers = { Authorization: `Bearer ${created.body.token}` };
    const byRoute = await request(`/api/dev/v1/accounts/${account.id}/messages?limit=10`, { headers });
    const byEmail = await request(`/api/dev/v1/messages?accountEmail=${encodeURIComponent(account.email)}`, { headers });
    const byProvider = await request('/api/dev/v1/messages?provider=gmail', { headers });
    expect(byRoute.body).toMatchObject({ total: 1, nextOffset: 1 });
    expect(byEmail.body.messages[0].id).toBe('message-dev');
    expect(byProvider.body.messages[0].accountId).toBe(account.id);
    expect((await request('/api/dev/v1/messages?accountEmail=missing@example.com', { headers })).response.status).toBe(404);
  });

  it('deletes account-owned cache and removes it from token grants', async () => {
    await request('/api/developer-tokens', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'Delete test', scopes: ['accounts:read'], accountIds: [account.id], ttlSeconds: 3600 }),
    });
    await updateStore((data) => { data.messages.push({
      id: 'message-1', accountId: account.id, mailbox: 'INBOX', uid: 1, from: { name: '', address: 'sender@example.com' }, to: [],
      subject: 'Subject', preview: '', text: '', date: '2026-07-28T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [],
    }); });
    const removed = await request(`/api/accounts/${account.id}`, { method: 'DELETE' });
    expect(removed.response.status).toBe(204);
    expect((await request('/api/accounts')).body.accounts).toEqual([]);
    expect((await request('/api/messages')).body.messages).toEqual([]);
    expect((await request('/api/developer-tokens')).body.tokens[0].accountIds).toEqual([]);
  });
});
