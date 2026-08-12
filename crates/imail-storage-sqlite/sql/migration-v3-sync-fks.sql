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
  reason TEXT NOT NULL CHECK (reason IN ('scheduled', 'startup', 'manual', 'recovery')),
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  priority INTEGER NOT NULL DEFAULT 0, not_before TEXT NOT NULL, locked_by TEXT, locked_until TEXT, attempts INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, synced_count INTEGER, new_count INTEGER, updated_count INTEGER, deleted_count INTEGER,
  error_code TEXT, error_message TEXT
) STRICT;
CREATE TABLE sync_events_with_fk (
  id INTEGER PRIMARY KEY AUTOINCREMENT, event_type TEXT NOT NULL,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  job_id TEXT REFERENCES sync_jobs_with_fk(id) ON DELETE SET NULL, payload_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL
) STRICT;
INSERT INTO sync_policies_with_fk SELECT p.* FROM sync_policies p JOIN accounts a ON a.id=p.account_id;
INSERT INTO mailbox_sync_states_with_fk SELECT s.* FROM mailbox_sync_states s JOIN accounts a ON a.id=s.account_id;
INSERT INTO sync_jobs_with_fk SELECT j.* FROM sync_jobs j JOIN accounts a ON a.id=j.account_id;
INSERT INTO sync_events_with_fk SELECT e.id, e.event_type, e.account_id,
  CASE WHEN EXISTS (SELECT 1 FROM sync_jobs_with_fk j WHERE j.id=e.job_id) THEN e.job_id ELSE NULL END,
  e.payload_json, e.created_at FROM sync_events e JOIN accounts a ON a.id=e.account_id;
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
