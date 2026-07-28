import { mkdtemp, rm } from 'node:fs/promises';
import type { Server } from 'node:http';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MailAccount } from './types.js';

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
