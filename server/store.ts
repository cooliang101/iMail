import { existsSync, mkdirSync, readFileSync, renameSync } from 'node:fs';
import path from 'node:path';
import { DatabaseSync, type StatementSync } from 'node:sqlite';
import { ensureSchema } from './storage/schema.js';
import { integer, messageFromRow, optionalText, text, type Row, type SqlValue } from './storage/rows.js';
import type { MessageQuery, MessageStats } from './storage/models.js';
import type { MailboxSyncCommit } from './storage/models.js';
import { readSnapshot } from './storage/snapshot.js';
import { replaceData } from './storage/write-data.js';
import type { CachedMessage, StoreData } from './types.js';
import { reconcileContacts } from './contact-model.js';
import { currentUserId, userMetadataKey } from './auth/context.js';

export type { MessageQuery, MessageStats } from './storage/models.js';

const initial: StoreData = { accounts: [], messages: [], tokens: [], drafts: [], contacts: [], logoFetchAttempts: [] };

export class SQLiteStore {
  private readonly db: DatabaseSync;
  private queue: Promise<void> = Promise.resolve();

  constructor(public readonly databasePath: string, private readonly legacyJsonPath?: string) {
    if (databasePath !== ':memory:') mkdirSync(path.dirname(databasePath), { recursive: true });
    this.db = new DatabaseSync(databasePath, { timeout: 5_000 });
    this.db.exec('PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;');
    if (databasePath !== ':memory:') this.db.exec('PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;');
    ensureSchema(this.db);
    this.migrateLegacyJson();
    this.materializeContactsForExistingDatabase();
  }

  private migrateLegacyJson() {
    if (!this.legacyJsonPath || !existsSync(this.legacyJsonPath)) return;
    const migrated = this.db.prepare("SELECT value FROM metadata WHERE key = 'legacy_json_migrated_at'").get() as Row | undefined;
    if (migrated) return;
    const counts = this.db.prepare(`SELECT
      (SELECT count(*) FROM accounts) + (SELECT count(*) FROM messages) + (SELECT count(*) FROM developer_tokens) AS total`).get() as Row;
    if (integer(counts, 'total') > 0) return;
    const parsed = JSON.parse(readFileSync(this.legacyJsonPath, 'utf8')) as Partial<StoreData>;
    const data: StoreData = {
      accounts: Array.isArray(parsed.accounts) ? parsed.accounts : [],
      messages: Array.isArray(parsed.messages) ? parsed.messages : [],
      tokens: Array.isArray(parsed.tokens) ? parsed.tokens : [],
      drafts: Array.isArray(parsed.drafts) ? parsed.drafts : [],
      contacts: Array.isArray(parsed.contacts) ? parsed.contacts : [],
      logoFetchAttempts: Array.isArray(parsed.logoFetchAttempts) ? parsed.logoFetchAttempts : [],
    };
    replaceData(this.db, data, new Date().toISOString());
    const migratedPath = `${this.legacyJsonPath}.migrated`;
    if (!existsSync(migratedPath)) renameSync(this.legacyJsonPath, migratedPath);
  }

  private materializeContactsForExistingDatabase() {
    const counts = this.db.prepare('SELECT (SELECT count(*) FROM messages) AS messages, (SELECT count(*) FROM contacts) AS contacts').get() as Row;
    if (integer(counts, 'messages') === 0 || integer(counts, 'contacts') > 0) return;
    replaceData(this.db, readSnapshot(this.db));
  }

  private all(statement: StatementSync, ...values: SqlValue[]): Row[] { return statement.all(...values) as Row[]; }

  async read(): Promise<StoreData> {
    await this.queue;
    return readSnapshot(this.db, currentUserId());
  }

  async listMessages(input: MessageQuery): Promise<{ messages: CachedMessage[]; total: number }> {
    await this.queue;
    const where: string[] = [];
    const values: SqlValue[] = [];
    const userId = currentUserId();
    if (userId) { where.push('a.user_id = ?'); values.push(userId); }
    if (input.accountId) { where.push('m.account_id = ?'); values.push(input.accountId); }
    if (input.group) { where.push('a.group_name = ?'); values.push(input.group); }
    if (input.unread) where.push('m.unread = 1');
    if (input.flagged) where.push('m.flagged = 1');
    if (input.hasAttachments) where.push('m.has_attachments = 1');
    if (input.mailboxRole) { where.push('m.mailbox_role = ?'); values.push(input.mailboxRole); }
    if (input.mailbox) { where.push('m.mailbox = ?'); values.push(input.mailbox); }
    if (input.mailboxName) {
      where.push("EXISTS (SELECT 1 FROM json_each(a.mailboxes_json) folder WHERE lower(json_extract(folder.value, '$.name')) = lower(?) AND json_extract(folder.value, '$.path') = m.mailbox)");
      values.push(input.mailboxName);
    }
    if (input.snoozed === true) where.push("m.snoozed_until IS NOT NULL AND m.snoozed_until > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
    else if (input.mailboxRole === 'inbox') where.push("(m.snoozed_until IS NULL OR m.snoozed_until <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))");
    if (input.label) { where.push('EXISTS (SELECT 1 FROM json_each(m.labels_json) WHERE value = ?)'); values.push(input.label); }
    if (input.query?.trim()) {
      where.push('(m.subject LIKE ? OR m.preview LIKE ? OR m.from_json LIKE ? OR m.to_json LIKE ?)');
      const pattern = `%${input.query.trim()}%`;
      values.push(pattern, pattern, pattern, pattern);
    }
    const clause = where.length ? `WHERE ${where.join(' AND ')}` : '';
    const from = `FROM messages m JOIN accounts a ON a.id = m.account_id ${clause}`;
    const count = this.db.prepare(`SELECT count(*) AS total ${from}`).get(...values) as Row;
    const rows = this.db.prepare(`SELECT m.* ${from} ORDER BY m.received_at DESC, m.id DESC LIMIT ? OFFSET ?`)
      .all(...values, input.limit, input.offset) as Row[];
    return { messages: rows.map(messageFromRow), total: integer(count, 'total') };
  }

  async getMessage(id: string): Promise<CachedMessage | undefined> {
    await this.queue;
    const userId = currentUserId();
    const row = (userId
      ? this.db.prepare('SELECT m.* FROM messages m JOIN accounts a ON a.id = m.account_id WHERE m.id = ? AND a.user_id = ?').get(id, userId)
      : this.db.prepare('SELECT * FROM messages WHERE id = ?').get(id)) as Row | undefined;
    return row ? messageFromRow(row) : undefined;
  }

  async messageStats(): Promise<MessageStats> {
    await this.queue;
    const userId = currentUserId();
    const owner = userId ? ' AND a.user_id = ?' : '';
    const activeInbox = "mailbox_role = 'inbox' AND (snoozed_until IS NULL OR snoozed_until <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))";
    const overall = this.db.prepare(`SELECT count(*) AS total, coalesce(sum(m.unread), 0) AS unread FROM messages m JOIN accounts a ON a.id = m.account_id WHERE m.${activeInbox}${owner}`).get(...(userId ? [userId] : [])) as Row;
    const byAccount = this.all(this.db.prepare(`SELECT account_id, count(*) AS total, coalesce(sum(unread), 0) AS unread
      FROM messages m JOIN accounts a ON a.id = m.account_id WHERE m.${activeInbox}${owner} GROUP BY account_id ORDER BY account_id`), ...(userId ? [userId] : [])).map((row) => ({
      accountId: text(row, 'account_id'), total: integer(row, 'total'), unread: integer(row, 'unread'),
    }));
    const byGroup = this.all(this.db.prepare(`SELECT a.group_name, count(*) AS total, coalesce(sum(m.unread), 0) AS unread
      FROM messages m JOIN accounts a ON a.id = m.account_id WHERE m.mailbox_role = 'inbox' AND (m.snoozed_until IS NULL OR m.snoozed_until <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))${owner} GROUP BY a.group_name ORDER BY a.group_name`), ...(userId ? [userId] : [])).map((row) => ({
      group: text(row, 'group_name'), total: integer(row, 'total'), unread: integer(row, 'unread'),
    }));
    return { total: integer(overall, 'total'), unread: integer(overall, 'unread'), byAccount, byGroup };
  }

  async getMetadata(key: string): Promise<string | undefined> {
    await this.queue;
    const row = this.db.prepare('SELECT value FROM metadata WHERE key = ?').get(key) as Row | undefined;
    return row ? text(row, 'value') : undefined;
  }

  async setMetadata(key: string, value: string): Promise<void> {
    const operation = this.queue.catch(() => undefined).then(() => {
      this.db.prepare('INSERT INTO metadata (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value').run(key, value);
    });
    this.queue = operation.then(() => undefined, () => undefined);
    await operation;
  }

  async update(mutator: (data: StoreData) => void | Promise<void>): Promise<StoreData> {
    let output = structuredClone(initial);
    const operation = this.queue.catch(() => undefined).then(async () => {
      this.db.exec('BEGIN IMMEDIATE');
      try {
        const userId = currentUserId();
        const data = readSnapshot(this.db, userId);
        await mutator(data);
        replaceData(this.db, data, undefined, false, userId);
        this.db.exec('COMMIT');
        output = data;
      } catch (error) {
        this.db.exec('ROLLBACK');
        throw error;
      }
    });
    this.queue = operation.then(() => undefined, () => undefined);
    await operation;
    return output;
  }

  async setAccountSyncStatus(accountId: string, status: 'connected' | 'syncing' | 'error', lastError?: string) {
    const operation = this.queue.catch(() => undefined).then(() => {
      this.db.prepare('UPDATE accounts SET status = ?, last_error = ? WHERE id = ?').run(status, lastError ?? null, accountId);
    });
    this.queue = operation.then(() => undefined, () => undefined);
    await operation;
  }

  async commitMailboxSync(input: MailboxSyncCommit): Promise<{ createdMessages: CachedMessage[] }> {
    let createdMessages: CachedMessage[] = [];
    const operation = this.queue.catch(() => undefined).then(() => {
      this.db.exec('BEGIN IMMEDIATE');
      try {
        const selectExisting = this.db.prepare('SELECT labels_json, snoozed_until FROM messages WHERE id = ?');
        const deleteMailbox = this.db.prepare('DELETE FROM messages WHERE account_id = ? AND mailbox = ?');
        const deleteUid = this.db.prepare('DELETE FROM messages WHERE account_id = ? AND mailbox = ? AND uid = ?');
        const deleteDuplicate = this.db.prepare(`DELETE FROM messages WHERE account_id = ? AND id <> ? AND message_id = ? AND
          (CASE WHEN ? = 'custom' THEN mailbox = ? ELSE mailbox_role = ? END)`);
        const upsert = this.db.prepare(`INSERT INTO messages
          (id, account_id, mailbox, mailbox_role, uid, message_id, from_json, to_json, subject, preview, text_body, html_body, received_at, unread, flagged, has_attachments, attachments_json, labels_json, snoozed_until)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
          ON CONFLICT(id) DO UPDATE SET mailbox = excluded.mailbox, mailbox_role = excluded.mailbox_role, uid = excluded.uid,
          message_id = excluded.message_id, from_json = excluded.from_json, to_json = excluded.to_json, subject = excluded.subject,
          preview = excluded.preview, text_body = excluded.text_body, html_body = excluded.html_body, received_at = excluded.received_at,
          unread = excluded.unread, flagged = excluded.flagged, has_attachments = excluded.has_attachments,
          attachments_json = excluded.attachments_json, labels_json = excluded.labels_json, snoozed_until = excluded.snoozed_until`);
        const updateFlags = this.db.prepare('UPDATE messages SET unread = ?, flagged = ? WHERE account_id = ? AND mailbox = ? AND uid = ?');
        const existingById = new Map<string, { labels: string[]; snoozedUntil?: string }>();
        for (const message of input.incoming) {
          const row = selectExisting.get(message.id) as Row | undefined;
          if (row) existingById.set(message.id, { labels: JSON.parse(text(row, 'labels_json')) as string[], snoozedUntil: optionalText(row, 'snoozed_until') });
          else createdMessages.push(message);
        }
        if (input.uidValidityChanged) deleteMailbox.run(input.accountId, input.mailbox);
        else for (const uid of input.removedUids) deleteUid.run(input.accountId, input.mailbox, uid);
        for (const message of input.incoming) {
          const previous = existingById.get(message.id);
          message.labels = previous?.labels ?? message.labels ?? [];
          message.snoozedUntil = previous?.snoozedUntil ?? message.snoozedUntil;
          if (message.messageId) deleteDuplicate.run(input.accountId, message.id, message.messageId, input.mailboxRole, input.mailbox, input.mailboxRole);
          upsert.run(message.id, message.accountId, message.mailbox, message.mailboxRole ?? 'inbox', message.uid, message.messageId ?? null,
            JSON.stringify(message.from), JSON.stringify(message.to), message.subject, message.preview, message.text, message.html ?? null,
            message.date, Number(message.unread), Number(message.flagged), Number(message.hasAttachments), JSON.stringify(message.attachments),
            JSON.stringify(message.labels ?? []), message.snoozedUntil ?? null);
        }
        for (const flags of input.flagUpdates) updateFlags.run(Number(flags.unread), Number(flags.flagged), input.accountId, input.mailbox, flags.uid);
        this.db.prepare(`DELETE FROM messages WHERE account_id = ? AND id NOT IN
          (SELECT id FROM messages WHERE account_id = ? ORDER BY received_at DESC, id DESC LIMIT 5000)`).run(input.accountId, input.accountId);
        this.db.prepare("UPDATE accounts SET status = 'connected', last_sync_at = ?, last_error = NULL, mailboxes_json = ? WHERE id = ?")
          .run(input.completedAt, JSON.stringify(input.folders), input.accountId);

        const owner = this.db.prepare('SELECT user_id FROM accounts WHERE id = ?').get(input.accountId) as { user_id: string } | undefined;
        const snapshot = readSnapshot(this.db, owner?.user_id);
        const contacts = reconcileContacts(snapshot);
        if (owner) this.db.prepare('DELETE FROM contacts WHERE user_id = ?').run(owner.user_id); else this.db.exec('DELETE FROM contacts');
        const insertContact = this.db.prepare('INSERT INTO contacts (address, name, message_count, last_contact_at, logo_key, logo_content_type, logo_source_url, logo_fetched_at, user_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)');
        for (const contact of contacts) insertContact.run(contact.address, contact.name, contact.messageCount, contact.lastContactAt, contact.logo?.key ?? null, contact.logo?.contentType ?? null, contact.logo?.sourceUrl ?? null, contact.logo?.fetchedAt ?? null, owner?.user_id ?? contact.ownerId ?? '__legacy__');
        this.db.exec('COMMIT');
      } catch (error) { this.db.exec('ROLLBACK'); throw error; }
    });
    this.queue = operation.then(() => undefined, () => undefined);
    await operation;
    return { createdMessages };
  }

  close() { this.db.close(); }
}

let defaultStore: SQLiteStore | undefined;
const auxiliaryStoreClosers = new Set<() => void>();

export function registerAuxiliaryStoreCloser(close: () => void) {
  auxiliaryStoreClosers.add(close);
  return () => auxiliaryStoreClosers.delete(close);
}

export function configuredDatabasePath() {
  return path.join(path.resolve(process.env.IMAIL_DATA_DIR ?? '.data'), 'imail.sqlite');
}

function configuredStore() {
  if (!defaultStore) {
    const dataDir = path.dirname(configuredDatabasePath());
    defaultStore = new SQLiteStore(configuredDatabasePath(), path.join(dataDir, 'store.json'));
  }
  return defaultStore;
}

export function readStore(): Promise<StoreData> { return configuredStore().read(); }
export function updateStore(mutator: (data: StoreData) => void | Promise<void>): Promise<StoreData> { return configuredStore().update(mutator); }
export function listCachedMessages(input: MessageQuery) { return configuredStore().listMessages(input); }
export function getCachedMessage(id: string) { return configuredStore().getMessage(id); }
export function getMessageStats() { return configuredStore().messageStats(); }
export function getMetadata(key: string) { return configuredStore().getMetadata(userMetadataKey(key)); }
export function setMetadata(key: string, value: string) { return configuredStore().setMetadata(userMetadataKey(key), value); }
export function setAccountSyncStatus(accountId: string, status: 'connected' | 'syncing' | 'error', lastError?: string) { return configuredStore().setAccountSyncStatus(accountId, status, lastError); }
export function commitMailboxSync(input: MailboxSyncCommit) { return configuredStore().commitMailboxSync(input); }
export function closeStore() {
  for (const close of [...auxiliaryStoreClosers]) close();
  auxiliaryStoreClosers.clear();
  defaultStore?.close(); defaultStore = undefined;
}
