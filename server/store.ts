import { existsSync, mkdirSync, readFileSync, renameSync } from 'node:fs';
import path from 'node:path';
import { DatabaseSync, type StatementSync } from 'node:sqlite';
import { ensureSchema } from './storage/schema.js';
import { integer, messageFromRow, text, type Row, type SqlValue } from './storage/rows.js';
import type { MessageQuery, MessageStats } from './storage/models.js';
import { readSnapshot } from './storage/snapshot.js';
import { replaceData } from './storage/write-data.js';
import type { CachedMessage, StoreData } from './types.js';

export type { MessageQuery, MessageStats } from './storage/models.js';

const initial: StoreData = { accounts: [], messages: [], tokens: [], drafts: [] };

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
    };
    replaceData(this.db, data, new Date().toISOString());
    const migratedPath = `${this.legacyJsonPath}.migrated`;
    if (!existsSync(migratedPath)) renameSync(this.legacyJsonPath, migratedPath);
  }

  private all(statement: StatementSync): Row[] { return statement.all() as Row[]; }

  async read(): Promise<StoreData> {
    await this.queue;
    return readSnapshot(this.db);
  }

  async listMessages(input: MessageQuery): Promise<{ messages: CachedMessage[]; total: number }> {
    await this.queue;
    const where: string[] = [];
    const values: SqlValue[] = [];
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
    const row = this.db.prepare('SELECT * FROM messages WHERE id = ?').get(id) as Row | undefined;
    return row ? messageFromRow(row) : undefined;
  }

  async messageStats(): Promise<MessageStats> {
    await this.queue;
    const activeInbox = "mailbox_role = 'inbox' AND (snoozed_until IS NULL OR snoozed_until <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))";
    const overall = this.db.prepare(`SELECT count(*) AS total, coalesce(sum(unread), 0) AS unread FROM messages WHERE ${activeInbox}`).get() as Row;
    const byAccount = this.all(this.db.prepare(`SELECT account_id, count(*) AS total, coalesce(sum(unread), 0) AS unread
      FROM messages WHERE ${activeInbox} GROUP BY account_id ORDER BY account_id`)).map((row) => ({
      accountId: text(row, 'account_id'), total: integer(row, 'total'), unread: integer(row, 'unread'),
    }));
    const byGroup = this.all(this.db.prepare(`SELECT a.group_name, count(*) AS total, coalesce(sum(m.unread), 0) AS unread
      FROM messages m JOIN accounts a ON a.id = m.account_id WHERE m.mailbox_role = 'inbox' AND (m.snoozed_until IS NULL OR m.snoozed_until <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) GROUP BY a.group_name ORDER BY a.group_name`)).map((row) => ({
      group: text(row, 'group_name'), total: integer(row, 'total'), unread: integer(row, 'unread'),
    }));
    return { total: integer(overall, 'total'), unread: integer(overall, 'unread'), byAccount, byGroup };
  }

  async update(mutator: (data: StoreData) => void | Promise<void>): Promise<StoreData> {
    let output = structuredClone(initial);
    const operation = this.queue.catch(() => undefined).then(async () => {
      const data = readSnapshot(this.db);
      await mutator(data);
      replaceData(this.db, data);
      output = data;
    });
    this.queue = operation.then(() => undefined, () => undefined);
    await operation;
    return output;
  }

  close() { this.db.close(); }
}

let defaultStore: SQLiteStore | undefined;

function configuredStore() {
  if (!defaultStore) {
    const dataDir = path.resolve(process.env.IMAIL_DATA_DIR ?? '.data');
    defaultStore = new SQLiteStore(path.join(dataDir, 'imail.sqlite'), path.join(dataDir, 'store.json'));
  }
  return defaultStore;
}

export function readStore(): Promise<StoreData> { return configuredStore().read(); }
export function updateStore(mutator: (data: StoreData) => void | Promise<void>): Promise<StoreData> { return configuredStore().update(mutator); }
export function listCachedMessages(input: MessageQuery) { return configuredStore().listMessages(input); }
export function getCachedMessage(id: string) { return configuredStore().getMessage(id); }
export function getMessageStats() { return configuredStore().messageStats(); }
export function closeStore() { defaultStore?.close(); defaultStore = undefined; }
