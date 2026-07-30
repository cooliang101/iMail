import { createServer, type Server } from 'node:http';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import WebSocket from 'ws';
import type { CachedMessage, MailAccount } from '../types.js';

let directory: string;
let server: Server;
let websocketUrl: string;
let account: MailAccount;
let otherAccount: MailAccount;
let rawToken: string;
let updateStore: typeof import('../store.js')['updateStore'];
let closeStore: typeof import('../store.js')['closeStore'];
let getSyncStore: typeof import('../sync/store.js')['getSyncStore'];
let withUserContext: typeof import('../auth/context.js')['withUserContext'];

function nextJson(socket: WebSocket) {
  return new Promise<any>((resolve, reject) => {
    socket.once('message', (data) => resolve(JSON.parse(data.toString())));
    socket.once('error', reject);
  });
}

function message(accountId: string, id: string): CachedMessage {
  return {
    id, accountId, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 2,
    from: { name: 'Sender', address: 'sender@example.com' }, to: [], subject: 'A new message', preview: 'Preview', text: 'Body',
    date: '2026-07-29T10:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [], labels: [],
  };
}

beforeAll(async () => {
  directory = await mkdtemp(path.join(tmpdir(), 'imail-ws-'));
  process.env.IMAIL_DATA_DIR = directory;
  process.env.APP_MASTER_KEY = '33'.repeat(32);
  vi.resetModules();
  const [{ createApp }, store, tokens, websocket, syncStore] = await Promise.all([
    import('../app.js'), import('../store.js'), import('../tokens.js'), import('./websocket.js'), import('../sync/store.js'),
  ]);
  updateStore = store.updateStore;
  closeStore = store.closeStore;
  getSyncStore = syncStore.getSyncStore;
  account = {
    id: crypto.randomUUID(), provider: 'gmail', email: 'owner@example.com', displayName: 'Owner', group: '个人', color: '#168f78',
    settings: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
    encryptedSecret: 'cipher', createdAt: new Date().toISOString(), lastSyncAt: new Date().toISOString(), status: 'connected',
  };
  otherAccount = { ...account, id: crypto.randomUUID(), email: 'other@example.com' };
  ({ withUserContext } = await import('../auth/context.js'));
  await withUserContext('websocket-test-user', () => updateStore((data) => { data.accounts = [account, otherAccount]; data.messages = []; data.tokens = []; }));
  rawToken = (await withUserContext('websocket-test-user', () => tokens.issueToken({ name: 'WebSocket test', scopes: ['messages:read'], accountIds: [account.id], ttlSeconds: 3600 }))).raw;
  server = createServer(createApp());
  websocket.attachGatewayWebSocket(server, { eventPollIntervalMs: 10 });
  server.listen(0, '127.0.0.1');
  await new Promise<void>((resolve) => server.once('listening', resolve));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('测试服务器启动失败');
  websocketUrl = `ws://127.0.0.1:${address.port}/gateway/v1/events`;
});

afterAll(async () => {
  await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  closeStore();
  delete process.env.IMAIL_DATA_DIR; delete process.env.APP_MASTER_KEY;
  await rm(directory, { recursive: true, force: true });
});

describe('developer gateway WebSocket', () => {
  it('authenticates in the first frame and only pushes events for authorized mailboxes', async () => {
    const socket = new WebSocket(websocketUrl);
    await new Promise<void>((resolve, reject) => { socket.once('open', resolve); socket.once('error', reject); });
    socket.send(JSON.stringify({ type: 'authenticate', token: rawToken }));
    await expect(nextJson(socket)).resolves.toMatchObject({ type: 'connected', occurredAt: expect.any(String) });

    getSyncStore().recordMessageCreated(otherAccount, [message(otherAccount.id, 'not-authorized')]);
    const received = nextJson(socket);
    getSyncStore().recordMessageCreated(account, [message(account.id, 'authorized-message')]);
    await expect(received).resolves.toMatchObject({
      id: expect.any(String), type: 'message.created', occurredAt: expect.any(String),
      data: { message: { id: 'authorized-message', accountEmail: account.email, subject: 'A new message' } },
    });
    expect(JSON.stringify(await received)).not.toContain(account.id);
    socket.close();
  });

  it('accepts an Authorization header and disconnects a revoked token', async () => {
    const socket = new WebSocket(websocketUrl, { headers: { Authorization: `Bearer ${rawToken}` } });
    await expect(nextJson(socket)).resolves.toMatchObject({ type: 'connected' });
    await withUserContext('websocket-test-user', () => updateStore((data) => { data.tokens = []; }));
    const closed = new Promise<number>((resolve) => socket.once('close', resolve));
    getSyncStore().recordMessageCreated(account, [message(account.id, 'after-revoke')]);
    await expect(closed).resolves.toBe(1008);
  });
});
