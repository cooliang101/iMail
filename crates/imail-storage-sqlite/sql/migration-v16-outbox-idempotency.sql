ALTER TABLE outbox_items ADD COLUMN idempotency_key TEXT;
ALTER TABLE outbox_items ADD COLUMN request_fingerprint TEXT;
CREATE UNIQUE INDEX outbox_owner_idempotency
  ON outbox_items(user_id, idempotency_key)
  WHERE idempotency_key IS NOT NULL;
UPDATE outbox_items
SET message_json=json_set(message_json,'$.text','','$.html',NULL,'$.attachments',NULL)
WHERE status IN ('sent','cancelled');
