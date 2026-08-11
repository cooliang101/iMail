DROP TABLE IF EXISTS accounts_per_user_email;
CREATE TABLE accounts_per_user_email (
  id TEXT PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL COLLATE NOCASE,
  display_name TEXT NOT NULL, group_name TEXT NOT NULL, group_icon TEXT NOT NULL DEFAULT 'folder', color TEXT NOT NULL,
  settings_json TEXT NOT NULL, encrypted_secret TEXT NOT NULL, auth_method TEXT, created_at TEXT NOT NULL,
  last_sync_at TEXT, status TEXT NOT NULL, last_error TEXT, mailboxes_json TEXT NOT NULL DEFAULT '[]',
  user_id TEXT NOT NULL DEFAULT '__legacy__', UNIQUE(user_id, email)
) STRICT;
INSERT INTO accounts_per_user_email SELECT id, provider, email, display_name, group_name, group_icon, color,
  settings_json, encrypted_secret, auth_method, created_at, last_sync_at, status, last_error, mailboxes_json, user_id FROM accounts;
DROP TABLE accounts;
ALTER TABLE accounts_per_user_email RENAME TO accounts;
