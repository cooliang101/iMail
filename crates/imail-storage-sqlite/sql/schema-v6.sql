CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS accounts (
  id TEXT PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL COLLATE NOCASE,
  display_name TEXT NOT NULL, group_name TEXT NOT NULL, group_icon TEXT NOT NULL DEFAULT 'folder', color TEXT NOT NULL,
  settings_json TEXT NOT NULL, proxy_json TEXT, encrypted_secret TEXT NOT NULL, auth_method TEXT, created_at TEXT NOT NULL,
  last_sync_at TEXT, status TEXT NOT NULL, last_error TEXT, mailboxes_json TEXT NOT NULL DEFAULT '[]',
  user_id TEXT NOT NULL DEFAULT '__legacy__', UNIQUE(user_id, email)
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
CREATE INDEX IF NOT EXISTS messages_date_id ON messages(received_at DESC, id DESC);
CREATE TABLE IF NOT EXISTS contacts (
  user_id TEXT NOT NULL DEFAULT '__legacy__', address TEXT NOT NULL COLLATE NOCASE, name TEXT NOT NULL,
  message_count INTEGER NOT NULL, last_contact_at TEXT NOT NULL, logo_key TEXT, logo_content_type TEXT,
  logo_source_url TEXT, logo_fetched_at TEXT, PRIMARY KEY (user_id, address)
) STRICT;
CREATE INDEX IF NOT EXISTS contacts_last_contact ON contacts(last_contact_at DESC);
CREATE TABLE IF NOT EXISTS logo_fetch_attempts (
  user_id TEXT NOT NULL DEFAULT '__legacy__', target TEXT NOT NULL, domain_key TEXT NOT NULL,
  status TEXT NOT NULL, detail TEXT NOT NULL, attempted_at TEXT NOT NULL, PRIMARY KEY (user_id, target)
) STRICT;
CREATE INDEX IF NOT EXISTS logo_fetch_attempts_domain ON logo_fetch_attempts(domain_key, attempted_at DESC);
CREATE TABLE IF NOT EXISTS drafts (
  id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  to_json TEXT NOT NULL, cc_json TEXT NOT NULL, subject TEXT NOT NULL, text_body TEXT NOT NULL,
  html_body TEXT NOT NULL DEFAULT '', attachments_json TEXT NOT NULL DEFAULT '[]',
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
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, PRIMARY KEY (token_id, account_id)
) STRICT;
CREATE TABLE IF NOT EXISTS sync_policies (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  folder_mode TEXT NOT NULL DEFAULT 'inbox' CHECK (folder_mode IN ('inbox', 'standard', 'selected')),
  selected_mailboxes_json TEXT NOT NULL DEFAULT '[]',
  notify_on_error INTEGER NOT NULL DEFAULT 1 CHECK (notify_on_error IN (0, 1)), updated_at TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS mailbox_sync_states (
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, mailbox TEXT NOT NULL,
  mailbox_role TEXT NOT NULL DEFAULT 'inbox', uid_validity TEXT, last_seen_uid INTEGER NOT NULL DEFAULT 0,
  highest_modseq TEXT, last_attempt_at TEXT, last_success_at TEXT, next_sync_at TEXT,
  consecutive_failures INTEGER NOT NULL DEFAULT 0,
  connection_status TEXT NOT NULL DEFAULT 'connected' CHECK (connection_status IN ('connected', 'unreachable', 'authRequired')),
  sync_state TEXT NOT NULL DEFAULT 'idle' CHECK (sync_state IN ('idle', 'scheduled', 'running', 'backoff', 'paused')),
  last_error_code TEXT, last_error_message TEXT, PRIMARY KEY (account_id, mailbox)
) STRICT;
CREATE INDEX IF NOT EXISTS mailbox_sync_states_due ON mailbox_sync_states(sync_state, next_sync_at);
CREATE TABLE IF NOT EXISTS sync_jobs (
  id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  mailbox TEXT, mailbox_role TEXT NOT NULL DEFAULT 'inbox',
  reason TEXT NOT NULL CHECK (reason IN ('scheduled', 'startup', 'manual', 'recovery')),
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  priority INTEGER NOT NULL DEFAULT 0, not_before TEXT NOT NULL, locked_by TEXT, locked_until TEXT,
  attempts INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT,
  synced_count INTEGER, new_count INTEGER, updated_count INTEGER, deleted_count INTEGER,
  error_code TEXT, error_message TEXT, rerun_requested INTEGER NOT NULL DEFAULT 0 CHECK (rerun_requested IN (0, 1))
) STRICT;
CREATE INDEX IF NOT EXISTS sync_jobs_claim ON sync_jobs(status, not_before, priority DESC, created_at);
CREATE UNIQUE INDEX IF NOT EXISTS sync_jobs_active_target ON sync_jobs(account_id, coalesce(mailbox, ''), mailbox_role)
  WHERE status IN ('queued', 'running');
CREATE TABLE IF NOT EXISTS sync_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT, event_type TEXT NOT NULL,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  job_id TEXT REFERENCES sync_jobs(id) ON DELETE SET NULL, payload_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS sync_events_created ON sync_events(created_at);
CREATE TABLE IF NOT EXISTS sync_worker_heartbeats (
  worker_id TEXT PRIMARY KEY, process_id INTEGER NOT NULL, host_name TEXT NOT NULL,
  started_at TEXT NOT NULL, heartbeat_at TEXT NOT NULL
) STRICT;
