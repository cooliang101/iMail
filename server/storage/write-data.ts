import type { DatabaseSync } from 'node:sqlite';
import type { StoreData } from '../types.js';

export function replaceData(db: DatabaseSync, data: StoreData, migratedAt?: string) {
  const insertAccount = db.prepare(`INSERT INTO accounts
    (id, provider, email, display_name, group_name, group_icon, color, settings_json, encrypted_secret, auth_method, created_at, last_sync_at, status, last_error, mailboxes_json)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`);
  const insertMessage = db.prepare(`INSERT INTO messages
    (id, account_id, mailbox, mailbox_role, uid, message_id, from_json, to_json, subject, preview, text_body, html_body, received_at, unread, flagged, has_attachments, attachments_json, labels_json, snoozed_until)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`);
  const insertDraft = db.prepare('INSERT INTO drafts (id, account_id, to_json, cc_json, subject, text_body, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)');
  const insertToken = db.prepare('INSERT INTO developer_tokens (id, name, token_hash, prefix, created_at, expires_at, last_used_at) VALUES (?, ?, ?, ?, ?, ?, ?)');
  const insertScope = db.prepare('INSERT INTO developer_token_scopes (token_id, scope) VALUES (?, ?)');
  const insertTokenAccount = db.prepare('INSERT INTO developer_token_accounts (token_id, account_id) VALUES (?, ?)');
  db.exec('BEGIN IMMEDIATE');
  try {
    db.exec('DELETE FROM developer_token_accounts; DELETE FROM developer_token_scopes; DELETE FROM developer_tokens; DELETE FROM drafts; DELETE FROM messages; DELETE FROM accounts;');
    for (const account of data.accounts) insertAccount.run(account.id, account.provider, account.email, account.displayName, account.group, account.groupIcon ?? 'folder', account.color, JSON.stringify(account.settings), account.encryptedSecret, account.authMethod ?? null, account.createdAt, account.lastSyncAt ?? null, account.status, account.lastError ?? null, JSON.stringify(account.mailboxes ?? []));
    for (const message of data.messages) insertMessage.run(message.id, message.accountId, message.mailbox, message.mailboxRole ?? 'inbox', message.uid, message.messageId ?? null, JSON.stringify(message.from), JSON.stringify(message.to), message.subject, message.preview, message.text, message.html ?? null, message.date, Number(message.unread), Number(message.flagged), Number(message.hasAttachments), JSON.stringify(message.attachments), JSON.stringify(message.labels ?? []), message.snoozedUntil ?? null);
    for (const draft of data.drafts ?? []) insertDraft.run(draft.id, draft.accountId, JSON.stringify(draft.to), JSON.stringify(draft.cc), draft.subject, draft.text, draft.createdAt, draft.updatedAt);
    for (const token of data.tokens) {
      insertToken.run(token.id, token.name, token.tokenHash, token.prefix, token.createdAt, token.expiresAt, token.lastUsedAt ?? null);
      for (const scope of token.scopes) insertScope.run(token.id, scope);
      for (const accountId of token.accountIds) insertTokenAccount.run(token.id, accountId);
    }
    if (migratedAt) db.prepare("INSERT OR REPLACE INTO metadata (key, value) VALUES ('legacy_json_migrated_at', ?)").run(migratedAt);
    db.exec('COMMIT');
  } catch (error) {
    db.exec('ROLLBACK');
    throw error;
  }
}
