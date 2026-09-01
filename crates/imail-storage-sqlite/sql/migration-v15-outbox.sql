CREATE TABLE IF NOT EXISTS outbox_items (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  message_json TEXT NOT NULL CHECK(json_valid(message_json)),
  draft_id TEXT,
  scheduled_at TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('scheduled','sending','sent','failed','needsReview','cancelled')),
  attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
  last_error_code TEXT,
  last_error_message TEXT,
  lease_until TEXT,
  sent_message_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  sent_at TEXT
) STRICT;
CREATE INDEX IF NOT EXISTS outbox_owner_schedule
  ON outbox_items(user_id, scheduled_at DESC, created_at DESC);
CREATE INDEX IF NOT EXISTS outbox_due
  ON outbox_items(status, scheduled_at, created_at)
  WHERE status='scheduled';
