CREATE TABLE smart_folders (
  user_id TEXT NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  id TEXT NOT NULL, name TEXT NOT NULL, filters_json TEXT NOT NULL,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  PRIMARY KEY(user_id, id)
) STRICT;

-- External content avoids a second full copy of all cached bodies.
CREATE VIRTUAL TABLE message_body_fts USING fts5(
  text_body, content='messages', content_rowid='rowid', tokenize='trigram'
);
CREATE TRIGGER messages_body_insert AFTER INSERT ON messages BEGIN
  INSERT INTO message_body_fts(rowid, text_body) VALUES(new.rowid, new.text_body);
END;
CREATE TRIGGER messages_body_delete AFTER DELETE ON messages BEGIN
  INSERT INTO message_body_fts(message_body_fts, rowid, text_body)
    VALUES('delete', old.rowid, old.text_body);
END;
CREATE TRIGGER messages_body_update AFTER UPDATE OF text_body ON messages
WHEN old.text_body IS NOT new.text_body BEGIN
  INSERT INTO message_body_fts(message_body_fts, rowid, text_body)
    VALUES('delete', old.rowid, old.text_body);
  INSERT INTO message_body_fts(rowid, text_body) VALUES(new.rowid, new.text_body);
END;
INSERT INTO message_body_fts(message_body_fts) VALUES('rebuild');
