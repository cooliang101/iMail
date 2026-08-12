DROP TABLE IF EXISTS sync_policies_push_first;
CREATE TABLE sync_policies_push_first (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  folder_mode TEXT NOT NULL DEFAULT 'inbox' CHECK (folder_mode IN ('inbox', 'standard', 'selected')),
  selected_mailboxes_json TEXT NOT NULL DEFAULT '[]',
  notify_on_error INTEGER NOT NULL DEFAULT 1 CHECK (notify_on_error IN (0, 1)), updated_at TEXT NOT NULL
) STRICT;
INSERT INTO sync_policies_push_first (account_id, enabled, folder_mode, selected_mailboxes_json, notify_on_error, updated_at)
  SELECT account_id, enabled, folder_mode, selected_mailboxes_json, notify_on_error, updated_at FROM sync_policies;
DROP TABLE sync_policies;
ALTER TABLE sync_policies_push_first RENAME TO sync_policies;
