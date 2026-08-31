ALTER TABLE messages ADD COLUMN mail_headers_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE drafts ADD COLUMN compose_json TEXT NOT NULL DEFAULT '{}';
