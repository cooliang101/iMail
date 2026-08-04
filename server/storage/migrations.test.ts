import { DatabaseSync } from 'node:sqlite';
import { afterEach, describe, expect, it } from 'vitest';
import { ensureSchema } from './schema.js';

const databases: DatabaseSync[] = [];
afterEach(() => { while (databases.length) databases.pop()!.close(); });

describe('versioned SQLite migrations', () => {
  it('refuses to open a database created by a newer service schema', () => {
    const db = new DatabaseSync(':memory:'); databases.push(db);
    db.exec("CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT; INSERT INTO metadata VALUES ('schema_version', '999')");
    expect(() => ensureSchema(db)).toThrow('schema v999 高于当前服务支持');
  });

  it('adds sync foreign keys without losing valid rows and cascades account deletion', () => {
    const db = new DatabaseSync(':memory:'); databases.push(db);
    db.exec(`
      PRAGMA foreign_keys = ON;
      CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
      INSERT INTO metadata VALUES ('schema_version', '2');
      CREATE TABLE accounts (
        id TEXT PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL COLLATE NOCASE, display_name TEXT NOT NULL,
        group_name TEXT NOT NULL, group_icon TEXT NOT NULL DEFAULT 'folder', color TEXT NOT NULL, settings_json TEXT NOT NULL,
        encrypted_secret TEXT NOT NULL, auth_method TEXT, created_at TEXT NOT NULL, last_sync_at TEXT, status TEXT NOT NULL,
        last_error TEXT, mailboxes_json TEXT NOT NULL DEFAULT '[]', user_id TEXT NOT NULL DEFAULT '__legacy__', UNIQUE(user_id, email)
      ) STRICT;
      INSERT INTO accounts VALUES ('account-1', 'gmail', 'owner@example.com', 'Owner', '个人', 'folder', '#168f78', '{}', 'cipher', 'oauth2', '2026-07-30T00:00:00.000Z', NULL, 'connected', NULL, '[]', 'user-1');
      CREATE TABLE sync_policies (account_id TEXT PRIMARY KEY, enabled INTEGER NOT NULL, interval_minutes INTEGER NOT NULL, folder_mode TEXT NOT NULL, selected_mailboxes_json TEXT NOT NULL, sync_on_start INTEGER NOT NULL, retry_on_recovery INTEGER NOT NULL, notify_on_error INTEGER NOT NULL, updated_at TEXT NOT NULL) STRICT;
      CREATE TABLE mailbox_sync_states (account_id TEXT NOT NULL, mailbox TEXT NOT NULL, mailbox_role TEXT NOT NULL, uid_validity TEXT, last_seen_uid INTEGER NOT NULL, highest_modseq TEXT, last_attempt_at TEXT, last_success_at TEXT, next_sync_at TEXT, consecutive_failures INTEGER NOT NULL, connection_status TEXT NOT NULL, sync_state TEXT NOT NULL, last_error_code TEXT, last_error_message TEXT, PRIMARY KEY (account_id, mailbox)) STRICT;
      CREATE TABLE sync_jobs (id TEXT PRIMARY KEY, account_id TEXT NOT NULL, mailbox TEXT, mailbox_role TEXT NOT NULL, reason TEXT NOT NULL, status TEXT NOT NULL, priority INTEGER NOT NULL, not_before TEXT NOT NULL, locked_by TEXT, locked_until TEXT, attempts INTEGER NOT NULL, created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, synced_count INTEGER, new_count INTEGER, updated_count INTEGER, deleted_count INTEGER, error_code TEXT, error_message TEXT) STRICT;
      CREATE TABLE sync_events (id INTEGER PRIMARY KEY AUTOINCREMENT, event_type TEXT NOT NULL, account_id TEXT NOT NULL, job_id TEXT, payload_json TEXT NOT NULL, created_at TEXT NOT NULL) STRICT;
      INSERT INTO sync_policies VALUES ('account-1', 1, 5, 'inbox', '[]', 1, 1, 1, '2026-07-30T00:00:00.000Z');
      INSERT INTO sync_jobs VALUES ('job-1', 'account-1', NULL, 'inbox', 'manual', 'succeeded', 0, '2026-07-30T00:00:00.000Z', NULL, NULL, 1, '2026-07-30T00:00:00.000Z', NULL, NULL, 0, 0, 0, 0, NULL, NULL);
      INSERT INTO sync_events (event_type, account_id, job_id, payload_json, created_at) VALUES ('sync.completed', 'account-1', 'job-1', '{}', '2026-07-30T00:00:00.000Z');
    `);

    ensureSchema(db);
    expect((db.prepare("SELECT value FROM metadata WHERE key = 'schema_version'").get() as { value: string }).value).toBe('4');
    expect((db.prepare("SELECT rerun_requested FROM sync_jobs WHERE id = 'job-1'").get() as { rerun_requested: number }).rerun_requested).toBe(0);
    expect((db.prepare('SELECT count(*) AS count FROM sync_events').get() as { count: number }).count).toBe(1);
    expect(db.prepare('PRAGMA foreign_key_check').all()).toEqual([]);
    db.prepare("DELETE FROM accounts WHERE id = 'account-1'").run();
    for (const table of ['sync_policies', 'sync_jobs', 'sync_events']) {
      expect((db.prepare(`SELECT count(*) AS count FROM ${table}`).get() as { count: number }).count).toBe(0);
    }
  });
});
