import { existsSync, mkdirSync, readFileSync, renameSync } from 'node:fs';
import path from 'node:path';
import { DatabaseSync, type StatementSync } from 'node:sqlite';
import type { CachedMessage, DeveloperToken, MailAccount, StoreData, TokenScope } from './types.js';

const initial: StoreData = { accounts: [], messages: [], tokens: [] };

type SqlValue = string | number | bigint | null | Uint8Array;
type Row = Record<string, SqlValue>;

export type MessageQuery = {
  accountId?: string;
  group?: string;
  query?: string;
  unread?: boolean;
  flagged?: boolean;
  hasAttachments?: boolean;
  limit: number;
  offset: number;
};

function text(row: Row, key: string) { return String(row[key] ?? ''); }
function optionalText(row: Row, key: string) { return row[key] === null || row[key] === undefined ? undefined : String(row[key]); }
function json<T>(row: Row, key: string): T { return JSON.parse(text(row, key)) as T; }
function integer(row: Row, key: string) { return Number(row[key]); }

function messageFromRow(row: Row): CachedMessage {
  return {
    id: text(row, 'id'), accountId: text(row, 'account_id'), mailbox: text(row, 'mailbox'), uid: integer(row, 'uid'),
    messageId: optionalText(row, 'message_id'), from: json(row, 'from_json'), to: json(row, 'to_json'),
    subject: text(row, 'subject'), preview: text(row, 'preview'), text: text(row, 'text_body'), html: optionalText(row, 'html_body'),
    date: text(row, 'received_at'), unread: Boolean(integer(row, 'unread')), flagged: Boolean(integer(row, 'flagged')),
    hasAttachments: Boolean(integer(row, 'has_attachments')), attachments: json(row, 'attachments_json'),
  };
}

export class SQLiteStore {
  private readonly db: DatabaseSync;
  private queue: Promise<void> = Promise.resolve();

  constructor(public readonly databasePath: string, private readonly legacyJsonPath?: string) {
    if (databasePath !== ':memory:') mkdirSync(path.dirname(databasePath), { recursive: true });
    this.db = new DatabaseSync(databasePath, { timeout: 5_000 });
    this.db.exec('PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;');
    if (databasePath !== ':memory:') this.db.exec('PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;');
    this.initializeSchema();
    this.migrateLegacyJson();
  }

  private initializeSchema() {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS metadata (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
      ) STRICT;

      CREATE TABLE IF NOT EXISTS accounts (
        id TEXT PRIMARY KEY,
        provider TEXT NOT NULL,
        email TEXT NOT NULL COLLATE NOCASE UNIQUE,
        display_name TEXT NOT NULL,
        group_name TEXT NOT NULL,
        color TEXT NOT NULL,
        settings_json TEXT NOT NULL,
        encrypted_secret TEXT NOT NULL,
        auth_method TEXT,
        created_at TEXT NOT NULL,
        last_sync_at TEXT,
        status TEXT NOT NULL,
        last_error TEXT
      ) STRICT;

      CREATE TABLE IF NOT EXISTS messages (
        id TEXT PRIMARY KEY,
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        mailbox TEXT NOT NULL,
        uid INTEGER NOT NULL,
        message_id TEXT,
        from_json TEXT NOT NULL,
        to_json TEXT NOT NULL,
        subject TEXT NOT NULL,
        preview TEXT NOT NULL,
        text_body TEXT NOT NULL,
        html_body TEXT,
        received_at TEXT NOT NULL,
        unread INTEGER NOT NULL CHECK (unread IN (0, 1)),
        flagged INTEGER NOT NULL CHECK (flagged IN (0, 1)),
        has_attachments INTEGER NOT NULL CHECK (has_attachments IN (0, 1)),
        attachments_json TEXT NOT NULL,
        UNIQUE(account_id, mailbox, uid)
      ) STRICT;
      CREATE INDEX IF NOT EXISTS messages_account_date ON messages(account_id, received_at DESC);
      CREATE INDEX IF NOT EXISTS messages_date ON messages(received_at DESC);

      CREATE TABLE IF NOT EXISTS developer_tokens (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        token_hash TEXT NOT NULL UNIQUE,
        prefix TEXT NOT NULL,
        created_at TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        last_used_at TEXT
      ) STRICT;

      CREATE TABLE IF NOT EXISTS developer_token_scopes (
        token_id TEXT NOT NULL REFERENCES developer_tokens(id) ON DELETE CASCADE,
        scope TEXT NOT NULL,
        PRIMARY KEY (token_id, scope)
      ) STRICT;

      CREATE TABLE IF NOT EXISTS developer_token_accounts (
        token_id TEXT NOT NULL REFERENCES developer_tokens(id) ON DELETE CASCADE,
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        PRIMARY KEY (token_id, account_id)
      ) STRICT;
    `);
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
    };
    this.replaceData(data, new Date().toISOString());
    const migratedPath = `${this.legacyJsonPath}.migrated`;
    if (!existsSync(migratedPath)) renameSync(this.legacyJsonPath, migratedPath);
  }

  private all(statement: StatementSync): Row[] { return statement.all() as Row[]; }

  private snapshot(): StoreData {
    const accounts = this.all(this.db.prepare('SELECT * FROM accounts ORDER BY created_at')).map((row): MailAccount => ({
      id: text(row, 'id'), provider: text(row, 'provider') as MailAccount['provider'], email: text(row, 'email'),
      displayName: text(row, 'display_name'), group: text(row, 'group_name'), color: text(row, 'color'),
      settings: json(row, 'settings_json'), encryptedSecret: text(row, 'encrypted_secret'),
      authMethod: optionalText(row, 'auth_method') as MailAccount['authMethod'], createdAt: text(row, 'created_at'),
      lastSyncAt: optionalText(row, 'last_sync_at'), status: text(row, 'status') as MailAccount['status'], lastError: optionalText(row, 'last_error'),
    }));
    const messages = this.all(this.db.prepare('SELECT * FROM messages ORDER BY received_at DESC')).map(messageFromRow);
    const scopeRows = this.all(this.db.prepare('SELECT token_id, scope FROM developer_token_scopes ORDER BY scope'));
    const accountRows = this.all(this.db.prepare('SELECT token_id, account_id FROM developer_token_accounts ORDER BY account_id'));
    const tokens = this.all(this.db.prepare('SELECT * FROM developer_tokens ORDER BY created_at DESC')).map((row): DeveloperToken => ({
      id: text(row, 'id'), name: text(row, 'name'), tokenHash: text(row, 'token_hash'), prefix: text(row, 'prefix'),
      scopes: scopeRows.filter((item) => text(item, 'token_id') === text(row, 'id')).map((item) => text(item, 'scope') as TokenScope),
      accountIds: accountRows.filter((item) => text(item, 'token_id') === text(row, 'id')).map((item) => text(item, 'account_id')),
      createdAt: text(row, 'created_at'), expiresAt: text(row, 'expires_at'), lastUsedAt: optionalText(row, 'last_used_at'),
    }));
    return { accounts, messages, tokens };
  }

  async read(): Promise<StoreData> {
    await this.queue;
    return this.snapshot();
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

  async update(mutator: (data: StoreData) => void | Promise<void>): Promise<StoreData> {
    let output = structuredClone(initial);
    const operation = this.queue.catch(() => undefined).then(async () => {
      const data = this.snapshot();
      await mutator(data);
      this.replaceData(data);
      output = data;
    });
    this.queue = operation.then(() => undefined, () => undefined);
    await operation;
    return output;
  }

  private replaceData(data: StoreData, migratedAt?: string) {
    const insertAccount = this.db.prepare(`INSERT INTO accounts
      (id, provider, email, display_name, group_name, color, settings_json, encrypted_secret, auth_method, created_at, last_sync_at, status, last_error)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`);
    const insertMessage = this.db.prepare(`INSERT INTO messages
      (id, account_id, mailbox, uid, message_id, from_json, to_json, subject, preview, text_body, html_body, received_at, unread, flagged, has_attachments, attachments_json)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`);
    const insertToken = this.db.prepare('INSERT INTO developer_tokens (id, name, token_hash, prefix, created_at, expires_at, last_used_at) VALUES (?, ?, ?, ?, ?, ?, ?)');
    const insertScope = this.db.prepare('INSERT INTO developer_token_scopes (token_id, scope) VALUES (?, ?)');
    const insertTokenAccount = this.db.prepare('INSERT INTO developer_token_accounts (token_id, account_id) VALUES (?, ?)');
    this.db.exec('BEGIN IMMEDIATE');
    try {
      this.db.exec('DELETE FROM developer_token_accounts; DELETE FROM developer_token_scopes; DELETE FROM developer_tokens; DELETE FROM messages; DELETE FROM accounts;');
      for (const account of data.accounts) insertAccount.run(account.id, account.provider, account.email, account.displayName, account.group, account.color, JSON.stringify(account.settings), account.encryptedSecret, account.authMethod ?? null, account.createdAt, account.lastSyncAt ?? null, account.status, account.lastError ?? null);
      for (const message of data.messages) insertMessage.run(message.id, message.accountId, message.mailbox, message.uid, message.messageId ?? null, JSON.stringify(message.from), JSON.stringify(message.to), message.subject, message.preview, message.text, message.html ?? null, message.date, Number(message.unread), Number(message.flagged), Number(message.hasAttachments), JSON.stringify(message.attachments));
      for (const token of data.tokens) {
        insertToken.run(token.id, token.name, token.tokenHash, token.prefix, token.createdAt, token.expiresAt, token.lastUsedAt ?? null);
        for (const scope of token.scopes) insertScope.run(token.id, scope);
        for (const accountId of token.accountIds) insertTokenAccount.run(token.id, accountId);
      }
      if (migratedAt) this.db.prepare("INSERT OR REPLACE INTO metadata (key, value) VALUES ('legacy_json_migrated_at', ?)").run(migratedAt);
      this.db.exec('COMMIT');
    } catch (error) {
      this.db.exec('ROLLBACK');
      throw error;
    }
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
export function closeStore() { defaultStore?.close(); defaultStore = undefined; }
