CREATE TABLE IF NOT EXISTS mail_work_items (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK(status IN ('needsReply','needsReview','followUp','waiting')),
  due_at TEXT,
  note TEXT NOT NULL DEFAULT '',
  draft_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, message_id)
) STRICT;
CREATE INDEX IF NOT EXISTS mail_work_items_owner_status_due
  ON mail_work_items(user_id, status, due_at, updated_at DESC);
