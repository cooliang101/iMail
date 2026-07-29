import type { DatabaseSync } from 'node:sqlite';

export function ensureSchema(db: DatabaseSync) {
  db.exec(`
    CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
    CREATE TABLE IF NOT EXISTS accounts (
      id TEXT PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL COLLATE NOCASE UNIQUE,
      display_name TEXT NOT NULL, group_name TEXT NOT NULL, color TEXT NOT NULL, settings_json TEXT NOT NULL,
      encrypted_secret TEXT NOT NULL, auth_method TEXT, created_at TEXT NOT NULL, last_sync_at TEXT,
      status TEXT NOT NULL, last_error TEXT
    ) STRICT;
    CREATE TABLE IF NOT EXISTS messages (
      id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
      mailbox TEXT NOT NULL, mailbox_role TEXT NOT NULL DEFAULT 'inbox', uid INTEGER NOT NULL, message_id TEXT,
      from_json TEXT NOT NULL, to_json TEXT NOT NULL, subject TEXT NOT NULL, preview TEXT NOT NULL,
      text_body TEXT NOT NULL, html_body TEXT, received_at TEXT NOT NULL,
      unread INTEGER NOT NULL CHECK (unread IN (0, 1)), flagged INTEGER NOT NULL CHECK (flagged IN (0, 1)),
      has_attachments INTEGER NOT NULL CHECK (has_attachments IN (0, 1)), attachments_json TEXT NOT NULL,
      labels_json TEXT NOT NULL DEFAULT '[]', snoozed_until TEXT, UNIQUE(account_id, mailbox, uid)
    ) STRICT;
    CREATE INDEX IF NOT EXISTS messages_account_date ON messages(account_id, received_at DESC);
    CREATE INDEX IF NOT EXISTS messages_date ON messages(received_at DESC);
    CREATE TABLE IF NOT EXISTS drafts (
      id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
      to_json TEXT NOT NULL, cc_json TEXT NOT NULL, subject TEXT NOT NULL, text_body TEXT NOT NULL,
      created_at TEXT NOT NULL, updated_at TEXT NOT NULL
    ) STRICT;
    CREATE TABLE IF NOT EXISTS developer_tokens (
      id TEXT PRIMARY KEY, name TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE, prefix TEXT NOT NULL,
      created_at TEXT NOT NULL, expires_at TEXT NOT NULL, last_used_at TEXT
    ) STRICT;
    CREATE TABLE IF NOT EXISTS developer_token_scopes (
      token_id TEXT NOT NULL REFERENCES developer_tokens(id) ON DELETE CASCADE,
      scope TEXT NOT NULL, PRIMARY KEY (token_id, scope)
    ) STRICT;
    CREATE TABLE IF NOT EXISTS developer_token_accounts (
      token_id TEXT NOT NULL REFERENCES developer_tokens(id) ON DELETE CASCADE,
      account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
      PRIMARY KEY (token_id, account_id)
    ) STRICT;
  `);
  const columns = new Set((db.prepare('PRAGMA table_info(messages)').all() as Array<Record<string, unknown>>).map((row) => String(row.name)));
  if (!columns.has('mailbox_role')) db.exec("ALTER TABLE messages ADD COLUMN mailbox_role TEXT NOT NULL DEFAULT 'inbox'");
  if (!columns.has('labels_json')) db.exec("ALTER TABLE messages ADD COLUMN labels_json TEXT NOT NULL DEFAULT '[]'");
  if (!columns.has('snoozed_until')) db.exec('ALTER TABLE messages ADD COLUMN snoozed_until TEXT');
}
