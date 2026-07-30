import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { SQLiteStore, listCachedMessages, readStore, updateStore } from './store.js';
import { withUserContext } from './auth/context.js';
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
  it('rejects public user-scoped storage operations without an explicit context', () => {
    expect(() => readStore()).toThrow('用户存储操作缺少明确的用户上下文');
    expect(() => updateStore(() => undefined)).toThrow('用户存储操作缺少明确的用户上下文');
    expect(() => listCachedMessages({ limit: 10, offset: 0 })).toThrow('用户存储操作缺少明确的用户上下文');
  });

  it('initializes an empty database', async () => {
    const { store } = await temporaryStore();
    expect(await store.read()).toEqual({ accounts: [], messages: [], tokens: [], drafts: [], contacts: [], logoFetchAttempts: [] });
  });

  it('round-trips accounts, nested messages, attachments and normalized token relations', async () => {
    const { store } = await temporaryStore();
    const expected = { accounts: [account({ lastSyncAt: '2026-07-28T02:00:00.000Z', groupIcon: 'briefcase' })], messages: [message()], tokens: [token({ lastUsedAt: '2026-07-28T03:00:00.000Z' })], drafts: [] };
    await store.update((data) => { Object.assign(data, expected); });
    expect(await store.read()).toEqual({ ...expected, contacts: [{ address: 'sender@example.com', name: 'Sender', messageCount: 1, lastContactAt: '2026-07-28T01:00:00.000Z' }], logoFetchAttempts: [] });
  });

  it('persists records after closing and reopening the database', async () => {
    const { store, directory, legacyPath } = await temporaryStore();
    await store.update((data) => { data.accounts.push(account()); });
    store.close(); stores.splice(stores.indexOf(store), 1);
    const reopened = new SQLiteStore(path.join(directory, 'imail.sqlite'), legacyPath); stores.push(reopened);
    expect((await reopened.read()).accounts).toEqual([account()]);
  });

  it('migrates the legacy global email constraint to a per-user constraint', async () => {
    const directory = await mkdtemp(path.join(tmpdir(), 'imail-store-')); directories.push(directory);
    const databasePath = path.join(directory, 'imail.sqlite');
    const { DatabaseSync } = await import('node:sqlite');
    const legacy = new DatabaseSync(databasePath);
    legacy.exec(`CREATE TABLE accounts (
      id TEXT PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL COLLATE NOCASE UNIQUE,
      display_name TEXT NOT NULL, group_name TEXT NOT NULL, group_icon TEXT NOT NULL DEFAULT 'folder', color TEXT NOT NULL, settings_json TEXT NOT NULL,
      encrypted_secret TEXT NOT NULL, auth_method TEXT, created_at TEXT NOT NULL, last_sync_at TEXT,
      status TEXT NOT NULL, last_error TEXT, mailboxes_json TEXT NOT NULL DEFAULT '[]', user_id TEXT NOT NULL DEFAULT '__legacy__'
    ) STRICT;`);
    legacy.close();
    const store = new SQLiteStore(databasePath); stores.push(store);
    await withUserContext('user-a', () => store.update((data) => { data.accounts.push(account()); }));
    await withUserContext('user-b', () => store.update((data) => { data.accounts.push(account({ id: 'account-b' })); }));
    expect((await withUserContext('user-b', () => store.read())).accounts[0].email).toBe(account().email);
    const migrated = new DatabaseSync(databasePath);
    expect((migrated.prepare("SELECT value FROM metadata WHERE key = 'schema_version'").get() as { value: string }).value).toBe('3');
    for (const table of ['sync_policies', 'mailbox_sync_states', 'sync_jobs', 'sync_events']) {
      expect((migrated.prepare(`PRAGMA foreign_key_list(${table})`).all() as Array<{ table: string; from: string; on_delete: string }>))
        .toEqual(expect.arrayContaining([expect.objectContaining({ table: 'accounts', from: 'account_id', on_delete: 'CASCADE' })]));
    }
    migrated.close();
  });

  it('persists metadata independently from snapshot updates', async () => {
    const { store, directory, legacyPath } = await temporaryStore();
    await store.setMetadata('app_preferences_v1', JSON.stringify({ defaultMessageView: 'rendered' }));
    await store.update((data) => { data.accounts.push(account()); });
    expect(await store.getMetadata('app_preferences_v1')).toContain('rendered');
    store.close(); stores.splice(stores.indexOf(store), 1);
    const reopened = new SQLiteStore(path.join(directory, 'imail.sqlite'), legacyPath); stores.push(reopened);
    expect(await reopened.getMetadata('app_preferences_v1')).toContain('rendered');
  });

  it('materializes contacts when opening a database created before the contacts table was populated', async () => {
    const { store, directory, legacyPath } = await temporaryStore();
    await store.update((data) => { data.accounts = [account()]; data.messages = [message()]; });
    store.close(); stores.splice(stores.indexOf(store), 1);
    const databasePath = path.join(directory, 'imail.sqlite');
    const { DatabaseSync } = await import('node:sqlite');
    const database = new DatabaseSync(databasePath); database.exec('DELETE FROM contacts'); database.close();
    const reopened = new SQLiteStore(databasePath, legacyPath); stores.push(reopened);
    expect((await reopened.read()).contacts).toEqual([{ address: 'sender@example.com', name: 'Sender', messageCount: 1, lastContactAt: '2026-07-28T01:00:00.000Z' }]);
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

  it('merges same-named mailbox folders within one workspace', async () => {
    const { store } = await temporaryStore();
    const first = account({ group: '项目', mailboxes: [{ path: 'Projects', name: 'Projects', delimiter: '/', selectable: true, subscribed: true }] });
    const second = account({ id: 'account-2', email: 'second@example.com', group: '项目', mailboxes: [{ path: 'Folders/Projects', name: 'projects', delimiter: '/', selectable: true, subscribed: true }] });
    const outside = account({ id: 'account-3', email: 'outside@example.com', group: '个人', mailboxes: [{ path: 'Projects', name: 'Projects', delimiter: '/', selectable: true, subscribed: true }] });
    await store.update((data) => {
      data.accounts = [first, second, outside];
      data.messages = [
        message({ id: 'project-1', accountId: first.id, mailbox: 'Projects', mailboxRole: 'custom', uid: 1 }),
        message({ id: 'project-2', accountId: second.id, mailbox: 'Folders/Projects', mailboxRole: 'custom', uid: 2 }),
        message({ id: 'outside-project', accountId: outside.id, mailbox: 'Projects', mailboxRole: 'custom', uid: 3 }),
      ];
    });

    const result = await store.listMessages({ group: '项目', mailboxName: 'PROJECTS', limit: 10, offset: 0 });
    expect(result.messages.map((item) => item.id)).toEqual(['project-2', 'project-1']);
  });

  it('serializes concurrent updates without losing writes', async () => {
    const { store } = await temporaryStore();
    await Promise.all(Array.from({ length: 20 }, (_, index) => store.update(async (data) => {
      await Promise.resolve();
      data.accounts.push(account({ id: `account-${index}`, email: `owner${index}@example.com` }));
    })));
    expect((await store.read()).accounts).toHaveLength(20);
  });

  it('allows separate application users to add the same mailbox address', async () => {
    const { store } = await temporaryStore();
    await withUserContext('user-a', () => store.update((data) => { data.accounts.push(account()); }));
    await withUserContext('user-b', () => store.update((data) => { data.accounts.push(account({ id: 'account-b' })); }));
    expect((await withUserContext('user-a', () => store.read())).accounts.map((item) => item.id)).toEqual([account().id]);
    expect((await withUserContext('user-b', () => store.read())).accounts.map((item) => item.id)).toEqual(['account-b']);
  });

  it('updates only changed records instead of rewriting the complete user snapshot', async () => {
    const { store, directory } = await temporaryStore();
    await withUserContext('user-a', () => store.update((data) => { data.accounts = [account()]; data.messages = [message()]; }));
    const { DatabaseSync } = await import('node:sqlite');
    const database = new DatabaseSync(path.join(directory, 'imail.sqlite'));
    database.exec('CREATE TABLE account_update_audit (count INTEGER NOT NULL); INSERT INTO account_update_audit VALUES (0); CREATE TRIGGER audit_account_update AFTER UPDATE ON accounts BEGIN UPDATE account_update_audit SET count = count + 1; END;');
    await withUserContext('user-a', () => store.update((data) => { data.messages[0].labels = ['changed']; }));
    expect((database.prepare('SELECT count FROM account_update_audit').get() as { count: number }).count).toBe(0);
    database.close();
  });

  it('commits mailbox synchronization with granular SQL while preserving local organization', async () => {
    const { store } = await temporaryStore();
    await store.update((data) => {
      data.accounts = [account()];
      data.messages = [
        message({ id: 'existing', uid: 42, labels: ['客户'], snoozedUntil: '2999-01-01T00:00:00.000Z' }),
        message({ id: 'deleted-remotely', uid: 43 }),
      ];
    });
    const refreshed = message({ id: 'existing', uid: 42, subject: 'Updated subject', unread: true, flagged: false, labels: [] });
    const incoming = message({ id: 'new-message', uid: 44, subject: 'New message', messageId: '<44@example.com>' });
    const result = await store.commitMailboxSync({
      accountId: account().id, mailbox: 'INBOX', mailboxRole: 'inbox', incoming: [refreshed, incoming], removedUids: [43], uidValidityChanged: false,
      flagUpdates: [{ uid: 42, unread: false, flagged: true }], folders: [{ path: 'INBOX', name: 'INBOX', delimiter: '/', selectable: true, subscribed: true }],
      completedAt: '2026-07-30T03:00:00.000Z',
    });
    expect(result.createdMessages.map((item) => item.id)).toEqual(['new-message']);
    expect(result.messageChanges).toEqual(expect.arrayContaining([
      expect.objectContaining({ before: expect.objectContaining({ id: 'deleted-remotely' }) }),
      expect.objectContaining({ before: expect.objectContaining({ id: 'existing' }), after: expect.objectContaining({ id: 'existing', unread: false, flagged: true }) }),
      expect.objectContaining({ after: expect.objectContaining({ id: 'new-message' }) }),
    ]));
    const snapshot = await store.read();
    expect(snapshot.messages.map((item) => item.id).sort()).toEqual(['existing', 'new-message']);
    expect(snapshot.messages.find((item) => item.id === 'existing')).toMatchObject({ subject: 'Updated subject', unread: false, flagged: true, labels: ['客户'], snoozedUntil: '2999-01-01T00:00:00.000Z' });
    expect(snapshot.accounts[0]).toMatchObject({ status: 'connected', lastSyncAt: '2026-07-30T03:00:00.000Z', mailboxes: [{ path: 'INBOX' }] });
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
      data.drafts = [{ id: 'draft-1', accountId: account().id, to: ['friend@example.com'], cc: [], subject: 'Draft', text: 'Body', html: '<p>Body</p>', attachments: [], createdAt: '2026-07-28T00:00:00.000Z', updatedAt: '2026-07-28T01:00:00.000Z' }];
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
    expect(await store.read()).toEqual({ accounts: [], messages: [], tokens: [], drafts: [], contacts: [], logoFetchAttempts: [] });
  });

  it('stores logos on contacts and shares one logo reference across a registrable domain', async () => {
    const { store } = await temporaryStore();
    await store.update((data) => {
      data.accounts = [account()];
      data.messages = [
        message({ id: 'google-root', uid: 1, from: { name: 'Google', address: 'noreply-accounts@google.com' } }),
        message({ id: 'google-accounts', uid: 2, from: { name: 'Google Accounts', address: 'no-reply@accounts.google.com' } }),
      ];
    });
    await store.update((data) => {
      const contact = data.contacts?.find((item) => item.address === 'noreply-accounts@google.com');
      if (contact) contact.logo = { key: 'domain:google.com', contentType: 'image/png', sourceUrl: 'https://google.com/favicon.ico', fetchedAt: '2026-07-30T00:00:00.000Z' };
    });
    const contacts = (await store.read()).contacts ?? [];
    expect(contacts).toHaveLength(2);
    expect(contacts.map((contact) => contact.logo?.key)).toEqual(['domain:google.com', 'domain:google.com']);
  });

  it('migrates legacy JSON once and preserves it as a migrated backup', async () => {
    const legacy = { accounts: [account()], messages: [message()], tokens: [token()], drafts: [] };
    const { store, directory, legacyPath } = await temporaryStore(legacy);
    expect(await store.read()).toEqual({ ...legacy, contacts: [{ address: 'sender@example.com', name: 'Sender', messageCount: 1, lastContactAt: '2026-07-28T01:00:00.000Z' }], logoFetchAttempts: [] });
    await expect(readFile(`${legacyPath}.migrated`, 'utf8')).resolves.toContain('owner@example.com');
    store.close(); stores.splice(stores.indexOf(store), 1);
    await writeFile(legacyPath, JSON.stringify({ accounts: [], messages: [], tokens: [] }), 'utf8');
    const reopened = new SQLiteStore(path.join(directory, 'imail.sqlite'), legacyPath); stores.push(reopened);
    expect((await reopened.read()).accounts).toHaveLength(1);
  });
});
