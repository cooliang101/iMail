import { mkdtemp, rm } from 'node:fs/promises';
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
  await updateStore((data) => { data.accounts = [account]; data.messages = []; data.tokens = []; data.drafts = []; });
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

  it('updates account workspace metadata without exposing credentials', async () => {
    const result = await request(`/api/accounts/${account.id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ group: '客户支持', groupIcon: 'users', displayName: 'Support' }) });
    expect(result.response.status).toBe(200);
    expect(result.body.account).toMatchObject({ group: '客户支持', groupIcon: 'users', displayName: 'Support' });
    expect(result.body.account).not.toHaveProperty('encryptedSecret');
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
    const archived = await request('/api/messages/message-lazy/move', {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ destination: 'archive' }),
    });
    expect(archived.response.status).toBe(200);
    expect(archived.body).toMatchObject({ destination: 'archive', mailbox: 'Archive', message: { id: 'message-lazy' } });
    expect((await request('/api/messages/message-lazy')).body.message).toMatchObject({ mailbox: 'Archive', mailboxRole: 'archive' });
    expect((await request('/api/messages?mailboxRole=archive')).body).toMatchObject({ total: 1, messages: [{ id: 'message-lazy', mailboxRole: 'archive' }] });
    expect((await request('/api/message-stats')).body).toMatchObject({ total: 0, unread: 0 });
  });

  it('persists drafts and exposes labels, snooze state and notifications', async () => {
    const createdDraft = await request('/api/drafts', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ accountId: account.id, to: ['friend@example.com'], subject: 'Draft subject', text: 'Draft body', html: '<p><strong>Draft body</strong></p>', attachments: [{ id: 'attachment-1', filename: 'note.txt', contentType: 'text/plain', size: 5, data: 'aGVsbG8=' }] }) });
    expect(createdDraft.response.status).toBe(201);
    expect((await request('/api/drafts')).body.drafts[0]).toMatchObject({ subject: 'Draft subject', to: ['friend@example.com'], html: '<p><strong>Draft body</strong></p>', attachments: [{ filename: 'note.txt', size: 5 }] });
    const draftId = createdDraft.body.draft.id;
    const updatedDraft = await request(`/api/drafts/${draftId}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ accountId: account.id, to: [], cc: [], subject: 'Updated draft', text: '' }) });
    expect(updatedDraft.body.draft.subject).toBe('Updated draft');

    await updateStore((data) => { data.messages = [{ id: 'organize-me', accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 10, from: { name: 'Sender', address: 'sender@example.com' }, to: [], subject: 'Organize', preview: '', text: 'Body', date: '2026-07-29T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [], labels: [] }]; });
    const organized = await request('/api/messages/organize-me', { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ labels: ['客户'], snoozedUntil: '2999-01-01T09:00:00.000Z' }) });
    expect(organized.body.message).toMatchObject({ labels: ['客户'], snoozedUntil: '2999-01-01T09:00:00.000Z' });
    expect((await request('/api/labels')).body.labels).toEqual(['客户']);
    expect((await request('/api/messages?mailboxRole=inbox')).body.total).toBe(0);
    expect((await request('/api/messages?mailboxRole=inbox&snoozed=true')).body.messages[0].id).toBe('organize-me');
    const notifications = await request('/api/notifications');
    expect(notifications.response.status).toBe(200);
    expect(Array.isArray(notifications.body.notifications)).toBe(true);
    expect((await request(`/api/drafts/${draftId}`, { method: 'DELETE' })).response.status).toBe(204);
    expect((await request('/api/drafts')).body.drafts).toEqual([]);
  });

  it('streams attachment downloads with safe response headers', async () => {
    const response = await fetch(`${baseUrl}/api/messages/message-file/attachments/0`);
    expect(response.status).toBe(200);
    expect(response.headers.get('content-type')).toContain('text/plain');
    expect(response.headers.get('content-disposition')).toContain("filename*=UTF-8''report.txt");
    expect(await response.text()).toBe('hello');
  });

  it('lets a developer token select a mailbox by email without exposing internal account IDs', async () => {
    await updateStore((data) => { data.messages = [
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
    expect((await fetch(`${baseUrl}/api/dev/v1/health`)).status).toBe(404);
  });

  it('deletes account-owned cache and removes it from token grants', async () => {
    await request('/api/developer-tokens', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'Delete test', scopes: ['accounts:read'], mailboxes: [account.email], ttlSeconds: 3600 }),
    });
    await updateStore((data) => { data.messages.push({
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
