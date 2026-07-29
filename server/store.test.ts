import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { SQLiteStore } from './store.js';
import type { CachedMessage, DeveloperToken, MailAccount, StoreData } from './types.js';

const directories: string[] = [];
const stores: SQLiteStore[] = [];

async function temporaryStore(legacy?: StoreData) {
  const directory = await mkdtemp(path.join(tmpdir(), 'imail-store-'));
  directories.push(directory);
  const legacyPath = path.join(directory, 'store.json');
  if (legacy) await writeFile(legacyPath, JSON.stringify(legacy), 'utf8');
  const store = new SQLiteStore(path.join(directory, 'imail.sqlite'), legacyPath);
  stores.push(store);
  return { store, directory, legacyPath };
}

function account(overrides: Partial<MailAccount> = {}): MailAccount {
  return {
    id: '11111111-1111-4111-8111-111111111111', provider: 'gmail', email: 'owner@example.com', displayName: 'Owner',
    group: '个人', color: '#168f78', settings: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
    encryptedSecret: 'ciphertext', authMethod: 'oauth2', createdAt: '2026-07-28T00:00:00.000Z', status: 'connected', ...overrides,
  };
}

function message(overrides: Partial<CachedMessage> = {}): CachedMessage {
  return {
    id: 'message-1', accountId: account().id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 42, messageId: '<42@example.com>',
    from: { name: 'Sender', address: 'sender@example.com' }, to: [{ name: 'Owner', address: 'owner@example.com' }],
    subject: 'Hello', preview: 'Preview', text: 'Body', html: '<p>Body</p>', date: '2026-07-28T01:00:00.000Z',
    unread: true, flagged: false, hasAttachments: true, attachments: [{ filename: 'note.txt', contentType: 'text/plain', size: 12, index: 0 }], labels: [], ...overrides,
  };
}

function token(overrides: Partial<DeveloperToken> = {}): DeveloperToken {
  return {
    id: '22222222-2222-4222-8222-222222222222', name: 'Tests', tokenHash: 'a'.repeat(64), prefix: 'imail_abc123',
    scopes: ['messages:read', 'messages:send'], accountIds: [account().id], createdAt: '2026-07-28T00:00:00.000Z',
    expiresAt: '2026-07-29T00:00:00.000Z', ...overrides,
  };
}

afterEach(async () => {
  while (stores.length) stores.pop()!.close();
  await Promise.all(directories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

describe('SQLiteStore', () => {
  it('initializes an empty database', async () => {
    const { store } = await temporaryStore();
    expect(await store.read()).toEqual({ accounts: [], messages: [], tokens: [], drafts: [] });
  });

  it('round-trips accounts, nested messages, attachments and normalized token relations', async () => {
    const { store } = await temporaryStore();
    const expected = { accounts: [account({ lastSyncAt: '2026-07-28T02:00:00.000Z' })], messages: [message()], tokens: [token({ lastUsedAt: '2026-07-28T03:00:00.000Z' })], drafts: [] };
    await store.update((data) => { Object.assign(data, expected); });
    expect(await store.read()).toEqual(expected);
  });

  it('persists records after closing and reopening the database', async () => {
    const { store, directory, legacyPath } = await temporaryStore();
    await store.update((data) => { data.accounts.push(account()); });
    store.close(); stores.splice(stores.indexOf(store), 1);
    const reopened = new SQLiteStore(path.join(directory, 'imail.sqlite'), legacyPath); stores.push(reopened);
    expect((await reopened.read()).accounts).toEqual([account()]);
  });

  it('paginates cached messages and applies account, group, search and status filters', async () => {
    const { store } = await temporaryStore();
    const second = account({ id: 'account-2', email: 'second@example.com', group: '工作' });
    await store.update((data) => {
      data.accounts = [account(), second];
      data.messages = [
        message({ id: 'm4', uid: 4, date: '2026-07-28T04:00:00.000Z', subject: 'Latest report', unread: true }),
        message({ id: 'm3', uid: 3, date: '2026-07-28T03:00:00.000Z', subject: 'Flagged', unread: false, flagged: true }),
        message({ id: 'm2', uid: 2, accountId: second.id, date: '2026-07-28T02:00:00.000Z', subject: 'Work invoice', hasAttachments: true }),
        message({ id: 'm1', uid: 1, accountId: second.id, date: '2026-07-28T01:00:00.000Z', subject: 'Old work' }),
      ];
    });
    const first = await store.listMessages({ limit: 2, offset: 0 });
    expect(first.total).toBe(4); expect(first.messages.map((item) => item.id)).toEqual(['m4', 'm3']);
    expect((await store.listMessages({ limit: 2, offset: 2 })).messages.map((item) => item.id)).toEqual(['m2', 'm1']);
    expect((await store.listMessages({ group: '工作', query: 'invoice', hasAttachments: true, limit: 10, offset: 0 })).messages.map((item) => item.id)).toEqual(['m2']);
    expect((await store.listMessages({ accountId: account().id, unread: true, limit: 10, offset: 0 })).messages.map((item) => item.id)).toEqual(['m4']);
    expect((await store.listMessages({ flagged: true, limit: 10, offset: 0 })).messages.map((item) => item.id)).toEqual(['m3']);
    expect(await store.messageStats()).toEqual({
      total: 4,
      unread: 3,
      byAccount: [
        { accountId: account().id, total: 2, unread: 1 },
        { accountId: second.id, total: 2, unread: 2 },
      ],
      byGroup: [
        { group: '个人', total: 2, unread: 1 },
        { group: '工作', total: 2, unread: 2 },
      ],
    });
  });

  it('serializes concurrent updates without losing writes', async () => {
    const { store } = await temporaryStore();
    await Promise.all(Array.from({ length: 20 }, (_, index) => store.update(async (data) => {
      await Promise.resolve();
      data.accounts.push(account({ id: `account-${index}`, email: `owner${index}@example.com` }));
    })));
    expect((await store.read()).accounts).toHaveLength(20);
  });

  it('persists drafts and filters mailbox roles, labels and snoozed messages', async () => {
    const { store } = await temporaryStore();
    await store.update((data) => {
      data.accounts = [account()];
      data.messages = [
        message({ id: 'inbox', uid: 1, labels: ['客户'] }),
        message({ id: 'snoozed', uid: 2, snoozedUntil: '2999-01-01T09:00:00.000Z' }),
        message({ id: 'archived', uid: 3, mailbox: 'Archive', mailboxRole: 'archive' }),
      ];
      data.drafts = [{ id: 'draft-1', accountId: account().id, to: ['friend@example.com'], cc: [], subject: 'Draft', text: 'Body', createdAt: '2026-07-28T00:00:00.000Z', updatedAt: '2026-07-28T01:00:00.000Z' }];
    });
    expect((await store.read()).drafts).toHaveLength(1);
    expect((await store.listMessages({ mailboxRole: 'inbox', limit: 10, offset: 0 })).messages.map((item) => item.id)).toEqual(['inbox']);
    expect((await store.listMessages({ mailboxRole: 'inbox', snoozed: true, limit: 10, offset: 0 })).messages.map((item) => item.id)).toEqual(['snoozed']);
    expect((await store.listMessages({ mailboxRole: 'archive', limit: 10, offset: 0 })).messages.map((item) => item.id)).toEqual(['archived']);
    expect((await store.listMessages({ label: '客户', limit: 10, offset: 0 })).messages.map((item) => item.id)).toEqual(['inbox']);
    expect(await store.messageStats()).toMatchObject({ total: 1, unread: 1 });
  });

  it('rolls back the complete transaction when a database constraint fails', async () => {
    const { store } = await temporaryStore();
    await store.update((data) => { data.accounts.push(account()); });
    await expect(store.update((data) => { data.accounts.push(account({ id: 'other', email: 'OWNER@example.com' })); })).rejects.toThrow();
    expect((await store.read()).accounts).toEqual([account()]);
  });

  it('enforces account references for cached messages and developer tokens', async () => {
    const { store } = await temporaryStore();
    await expect(store.update((data) => { data.messages.push(message({ accountId: 'missing' })); })).rejects.toThrow();
    await expect(store.update((data) => { data.tokens.push(token({ accountIds: ['missing'] })); })).rejects.toThrow();
    expect(await store.read()).toEqual({ accounts: [], messages: [], tokens: [], drafts: [] });
  });

  it('migrates legacy JSON once and preserves it as a migrated backup', async () => {
    const legacy = { accounts: [account()], messages: [message()], tokens: [token()], drafts: [] };
    const { store, directory, legacyPath } = await temporaryStore(legacy);
    expect(await store.read()).toEqual(legacy);
    await expect(readFile(`${legacyPath}.migrated`, 'utf8')).resolves.toContain('owner@example.com');
    store.close(); stores.splice(stores.indexOf(store), 1);
    await writeFile(legacyPath, JSON.stringify({ accounts: [], messages: [], tokens: [] }), 'utf8');
    const reopened = new SQLiteStore(path.join(directory, 'imail.sqlite'), legacyPath); stores.push(reopened);
    expect((await reopened.read()).accounts).toHaveLength(1);
  });
});
