CREATE TABLE IF NOT EXISTS mail_rules (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL, revision INTEGER NOT NULL,
  input_json TEXT NOT NULL CHECK(json_valid(input_json)),
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS mail_rules_owner ON mail_rules(user_id, created_at, id);
CREATE TABLE IF NOT EXISTS mail_rule_runs (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE,
  user_id TEXT NOT NULL, rule_id TEXT NOT NULL, rule_name TEXT NOT NULL,
  revision INTEGER NOT NULL, account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  message_id TEXT NOT NULL, identity TEXT NOT NULL, source TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('pending','running','succeeded','failed','needsReview','cancelled')),
  actions_json TEXT NOT NULL CHECK(json_valid(actions_json)),
  completed_actions INTEGER NOT NULL DEFAULT 0, local_actions INTEGER NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0, error_code TEXT, lease_until TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  UNIQUE(user_id,rule_id,revision,account_id,identity)
) STRICT;
CREATE INDEX IF NOT EXISTS mail_rule_runs_queue ON mail_rule_runs(account_id,status,sequence);
CREATE INDEX IF NOT EXISTS mail_rule_runs_owner ON mail_rule_runs(user_id,sequence DESC);
CREATE TABLE IF NOT EXISTS mail_rule_message_state (
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  identity TEXT NOT NULL, muted INTEGER NOT NULL CHECK(muted IN (0,1)),
  PRIMARY KEY(account_id,identity)
) STRICT;
CREATE TABLE IF NOT EXISTS mail_rule_previews (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL, rule_id TEXT NOT NULL,
  revision INTEGER NOT NULL, messages_json TEXT NOT NULL CHECK(json_valid(messages_json)),
  expires_at TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS mail_rule_previews_expiry ON mail_rule_previews(expires_at);
-- Existing cached mail is historical, including after a future IMAP UID reset.
INSERT OR IGNORE INTO mail_rule_message_state(account_id,identity,muted)
SELECT account_id,CASE WHEN message_id IS NOT NULL AND trim(message_id)<>''
  THEN 'rfc:' || message_id ELSE 'local:' || id END,0 FROM messages;
