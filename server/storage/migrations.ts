import type { DatabaseSync } from 'node:sqlite';
import packageMetadata from '../../package.json' with { type: 'json' };

export const CURRENT_SCHEMA_VERSION = packageMetadata.imail.schemaVersion;
type Row = Record<string, unknown>;

function columns(db: DatabaseSync, table: string) {
  return new Set((db.prepare(`PRAGMA table_info(${table})`).all() as Row[]).map((row) => String(row.name)));
}

function migrateLegacyColumns(db: DatabaseSync) {
  const messageColumns = columns(db, 'messages');
  if (!messageColumns.has('mailbox_role')) db.exec("ALTER TABLE messages ADD COLUMN mailbox_role TEXT NOT NULL DEFAULT 'inbox'");
  if (!messageColumns.has('labels_json')) db.exec("ALTER TABLE messages ADD COLUMN labels_json TEXT NOT NULL DEFAULT '[]'");
  if (!messageColumns.has('snoozed_until')) db.exec('ALTER TABLE messages ADD COLUMN snoozed_until TEXT');
  const accountColumns = columns(db, 'accounts');
  if (!accountColumns.has('user_id')) db.exec("ALTER TABLE accounts ADD COLUMN user_id TEXT NOT NULL DEFAULT '__legacy__'");
  if (!accountColumns.has('mailboxes_json')) db.exec("ALTER TABLE accounts ADD COLUMN mailboxes_json TEXT NOT NULL DEFAULT '[]'");
  if (!accountColumns.has('group_icon')) db.exec("ALTER TABLE accounts ADD COLUMN group_icon TEXT NOT NULL DEFAULT 'folder'");
  const draftColumns = columns(db, 'drafts');
  if (!draftColumns.has('html_body')) db.exec("ALTER TABLE drafts ADD COLUMN html_body TEXT NOT NULL DEFAULT ''");
  if (!draftColumns.has('attachments_json')) db.exec("ALTER TABLE drafts ADD COLUMN attachments_json TEXT NOT NULL DEFAULT '[]'");
  if (!columns(db, 'developer_tokens').has('user_id')) db.exec("ALTER TABLE developer_tokens ADD COLUMN user_id TEXT NOT NULL DEFAULT '__legacy__'");
  if (!columns(db, 'contacts').has('user_id')) db.exec(`
    ALTER TABLE contacts RENAME TO contacts_legacy_owner;
    CREATE TABLE contacts (user_id TEXT NOT NULL, address TEXT NOT NULL COLLATE NOCASE, name TEXT NOT NULL, message_count INTEGER NOT NULL,
      last_contact_at TEXT NOT NULL, logo_key TEXT, logo_content_type TEXT, logo_source_url TEXT, logo_fetched_at TEXT, PRIMARY KEY (user_id, address)) STRICT;
    INSERT INTO contacts SELECT '__legacy__', address, name, message_count, last_contact_at, logo_key, logo_content_type, logo_source_url, logo_fetched_at FROM contacts_legacy_owner;
    DROP TABLE contacts_legacy_owner;
    CREATE INDEX contacts_last_contact ON contacts(user_id, last_contact_at DESC);
  `);
  if (!columns(db, 'logo_fetch_attempts').has('user_id')) db.exec(`
    ALTER TABLE logo_fetch_attempts RENAME TO logo_fetch_attempts_legacy_owner;
    CREATE TABLE logo_fetch_attempts (user_id TEXT NOT NULL, target TEXT NOT NULL, domain_key TEXT NOT NULL, status TEXT NOT NULL,
      detail TEXT NOT NULL, attempted_at TEXT NOT NULL, PRIMARY KEY (user_id, target)) STRICT;
    INSERT INTO logo_fetch_attempts SELECT '__legacy__', target, domain_key, status, detail, attempted_at FROM logo_fetch_attempts_legacy_owner;
    DROP TABLE logo_fetch_attempts_legacy_owner;
    CREATE INDEX logo_fetch_attempts_domain ON logo_fetch_attempts(user_id, domain_key, attempted_at DESC);
  `);
}

function transactionWithoutForeignKeys(db: DatabaseSync, migrate: () => void) {
  db.exec('PRAGMA foreign_keys = OFF; BEGIN IMMEDIATE;');
  try { migrate(); db.exec('COMMIT;'); }
  catch (error) {
    try { db.exec('ROLLBACK;'); } catch { /* Preserve the migration error. */ }
    throw error;
  } finally { db.exec('PRAGMA foreign_keys = ON;'); }
}

function immediateTransaction(db: DatabaseSync, migrate: () => void) {
  db.exec('BEGIN IMMEDIATE;');
  try { migrate(); db.exec('COMMIT;'); }
  catch (error) {
    try { db.exec('ROLLBACK;'); } catch { /* Preserve the migration error. */ }
    throw error;
  }
}

function hasLegacyEmailIndex(db: DatabaseSync) {
  return (db.prepare('PRAGMA index_list(accounts)').all() as Row[]).some((index) => {
    if (!Number(index.unique)) return false;
    const columns = (db.prepare(`PRAGMA index_info(${JSON.stringify(String(index.name))})`).all() as Row[]).map((row) => String(row.name));
    return columns.length === 1 && columns[0] === 'email';
  });
}

function migrateAccountEmailConstraint(db: DatabaseSync) {
  if (!hasLegacyEmailIndex(db)) return;
  transactionWithoutForeignKeys(db, () => {
    if (!hasLegacyEmailIndex(db)) return;
    db.exec(`
      DROP TABLE IF EXISTS accounts_per_user_email;
      CREATE TABLE accounts_per_user_email (
        id TEXT PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL COLLATE NOCASE,
        display_name TEXT NOT NULL, group_name TEXT NOT NULL, group_icon TEXT NOT NULL DEFAULT 'folder', color TEXT NOT NULL, settings_json TEXT NOT NULL,
        encrypted_secret TEXT NOT NULL, auth_method TEXT, created_at TEXT NOT NULL, last_sync_at TEXT,
        status TEXT NOT NULL, last_error TEXT, mailboxes_json TEXT NOT NULL DEFAULT '[]', user_id TEXT NOT NULL DEFAULT '__legacy__',
        UNIQUE(user_id, email)
      ) STRICT;
      INSERT INTO accounts_per_user_email SELECT id, provider, email, display_name, group_name, group_icon, color, settings_json,
        encrypted_secret, auth_method, created_at, last_sync_at, status, last_error, mailboxes_json, user_id FROM accounts;
      DROP TABLE accounts;
      ALTER TABLE accounts_per_user_email RENAME TO accounts;
    `);
  });
}

function syncTablesHaveAccountForeignKeys(db: DatabaseSync) {
  return ['sync_policies', 'mailbox_sync_states', 'sync_jobs', 'sync_events'].every((table) =>
    (db.prepare(`PRAGMA foreign_key_list(${table})`).all() as Row[]).some((row) => String(row.table) === 'accounts' && String(row.from) === 'account_id' && String(row.on_delete).toUpperCase() === 'CASCADE'));
}

function migrateSyncForeignKeys(db: DatabaseSync) {
  if (syncTablesHaveAccountForeignKeys(db)) return;
  transactionWithoutForeignKeys(db, () => {
    if (syncTablesHaveAccountForeignKeys(db)) return;
    db.exec(`
      DROP TABLE IF EXISTS sync_policies_with_fk;
      DROP TABLE IF EXISTS mailbox_sync_states_with_fk;
      DROP TABLE IF EXISTS sync_jobs_with_fk;
      DROP TABLE IF EXISTS sync_events_with_fk;
      CREATE TABLE sync_policies_with_fk (
        account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
        enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)), interval_minutes INTEGER NOT NULL DEFAULT 5 CHECK (interval_minutes BETWEEN 1 AND 60),
        folder_mode TEXT NOT NULL DEFAULT 'inbox' CHECK (folder_mode IN ('inbox', 'standard', 'selected')), selected_mailboxes_json TEXT NOT NULL DEFAULT '[]',
        sync_on_start INTEGER NOT NULL DEFAULT 1 CHECK (sync_on_start IN (0, 1)), retry_on_recovery INTEGER NOT NULL DEFAULT 1 CHECK (retry_on_recovery IN (0, 1)),
        notify_on_error INTEGER NOT NULL DEFAULT 1 CHECK (notify_on_error IN (0, 1)), updated_at TEXT NOT NULL
      ) STRICT;
      CREATE TABLE mailbox_sync_states_with_fk (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, mailbox TEXT NOT NULL, mailbox_role TEXT NOT NULL DEFAULT 'inbox',
        uid_validity TEXT, last_seen_uid INTEGER NOT NULL DEFAULT 0, highest_modseq TEXT, last_attempt_at TEXT, last_success_at TEXT, next_sync_at TEXT,
        consecutive_failures INTEGER NOT NULL DEFAULT 0, connection_status TEXT NOT NULL DEFAULT 'connected' CHECK (connection_status IN ('connected', 'unreachable', 'authRequired')),
        sync_state TEXT NOT NULL DEFAULT 'idle' CHECK (sync_state IN ('idle', 'scheduled', 'running', 'backoff', 'paused')),
        last_error_code TEXT, last_error_message TEXT, PRIMARY KEY (account_id, mailbox)
      ) STRICT;
      CREATE TABLE sync_jobs_with_fk (
        id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, mailbox TEXT, mailbox_role TEXT NOT NULL DEFAULT 'inbox',
        reason TEXT NOT NULL CHECK (reason IN ('scheduled', 'startup', 'manual', 'recovery')), status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
        priority INTEGER NOT NULL DEFAULT 0, not_before TEXT NOT NULL, locked_by TEXT, locked_until TEXT, attempts INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, synced_count INTEGER, new_count INTEGER, updated_count INTEGER, deleted_count INTEGER,
        error_code TEXT, error_message TEXT
      ) STRICT;
      CREATE TABLE sync_events_with_fk (
        id INTEGER PRIMARY KEY AUTOINCREMENT, event_type TEXT NOT NULL, account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        job_id TEXT REFERENCES sync_jobs_with_fk(id) ON DELETE SET NULL, payload_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL
      ) STRICT;
      INSERT INTO sync_policies_with_fk SELECT p.* FROM sync_policies p JOIN accounts a ON a.id = p.account_id;
      INSERT INTO mailbox_sync_states_with_fk SELECT s.* FROM mailbox_sync_states s JOIN accounts a ON a.id = s.account_id;
      INSERT INTO sync_jobs_with_fk SELECT j.* FROM sync_jobs j JOIN accounts a ON a.id = j.account_id;
      INSERT INTO sync_events_with_fk SELECT e.id, e.event_type, e.account_id,
        CASE WHEN EXISTS (SELECT 1 FROM sync_jobs_with_fk j WHERE j.id = e.job_id) THEN e.job_id ELSE NULL END,
        e.payload_json, e.created_at FROM sync_events e JOIN accounts a ON a.id = e.account_id;
      DROP TABLE sync_events;
      DROP TABLE sync_jobs;
      DROP TABLE mailbox_sync_states;
      DROP TABLE sync_policies;
      ALTER TABLE sync_policies_with_fk RENAME TO sync_policies;
      ALTER TABLE mailbox_sync_states_with_fk RENAME TO mailbox_sync_states;
      ALTER TABLE sync_jobs_with_fk RENAME TO sync_jobs;
      ALTER TABLE sync_events_with_fk RENAME TO sync_events;
      CREATE INDEX mailbox_sync_states_due ON mailbox_sync_states(sync_state, next_sync_at);
      CREATE INDEX sync_jobs_claim ON sync_jobs(status, not_before, priority DESC, created_at);
      CREATE UNIQUE INDEX sync_jobs_active_target ON sync_jobs(account_id, coalesce(mailbox, ''), mailbox_role) WHERE status IN ('queued', 'running');
      CREATE INDEX sync_events_created ON sync_events(created_at);
    `);
  });
}

function migrateSyncJobWakeups(db: DatabaseSync) {
  if (!columns(db, 'sync_jobs').has('rerun_requested')) {
    db.exec('ALTER TABLE sync_jobs ADD COLUMN rerun_requested INTEGER NOT NULL DEFAULT 0 CHECK (rerun_requested IN (0, 1))');
  }
}

function migrateAccountProxyStorage(db: DatabaseSync) {
  immediateTransaction(db, () => {
    // API and worker processes can open the same v4 database concurrently. Recheck
    // after taking the write lock so exactly one connection performs the ALTER.
    if (!columns(db, 'accounts').has('proxy_json')) db.exec('ALTER TABLE accounts ADD COLUMN proxy_json TEXT');
  });
}

export function runMigrations(db: DatabaseSync) {
  const row = db.prepare("SELECT value FROM metadata WHERE key = 'schema_version'").get() as { value?: string } | undefined;
  const version = Number(row?.value ?? 0);
  if (!Number.isSafeInteger(version) || version < 0) throw new Error('iMail 数据库 schema 版本无效');
  if (version > CURRENT_SCHEMA_VERSION) {
    throw new Error(`数据库 schema v${version} 高于当前服务支持的 v${CURRENT_SCHEMA_VERSION}，请升级服务或恢复兼容快照`);
  }
  if (version < 1) migrateLegacyColumns(db);
  if (version < 2) migrateAccountEmailConstraint(db);
  if (version < 3) migrateSyncForeignKeys(db);
  if (version < 4) migrateSyncJobWakeups(db);
  if (version < 5) migrateAccountProxyStorage(db);
  if (version < CURRENT_SCHEMA_VERSION) {
    db.prepare("INSERT INTO metadata (key, value) VALUES ('schema_version', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
      .run(String(CURRENT_SCHEMA_VERSION));
  }
}
