import type { DatabaseSync } from 'node:sqlite';
import type { DeveloperToken, Draft, MailAccount, StoreData, TokenScope } from '../types.js';
import { json, messageFromRow, optionalText, text, type Row } from './rows.js';

function all(db: DatabaseSync, sql: string) { return db.prepare(sql).all() as Row[]; }

export function readSnapshot(db: DatabaseSync): StoreData {
  const accounts = all(db, 'SELECT * FROM accounts ORDER BY created_at').map((row): MailAccount => {
    const account: MailAccount = {
      id: text(row, 'id'), provider: text(row, 'provider') as MailAccount['provider'], email: text(row, 'email'),
      displayName: text(row, 'display_name'), group: text(row, 'group_name'), color: text(row, 'color'),
      settings: json(row, 'settings_json'), encryptedSecret: text(row, 'encrypted_secret'),
      authMethod: optionalText(row, 'auth_method') as MailAccount['authMethod'], createdAt: text(row, 'created_at'),
      status: text(row, 'status') as MailAccount['status'],
    };
    const lastSyncAt = optionalText(row, 'last_sync_at'); if (lastSyncAt) account.lastSyncAt = lastSyncAt;
    const lastError = optionalText(row, 'last_error'); if (lastError) account.lastError = lastError;
    return account;
  });
  const messages = all(db, 'SELECT * FROM messages ORDER BY received_at DESC').map(messageFromRow);
  const drafts = all(db, 'SELECT * FROM drafts ORDER BY updated_at DESC').map((row): Draft => ({
    id: text(row, 'id'), accountId: text(row, 'account_id'), to: json(row, 'to_json'), cc: json(row, 'cc_json'),
    subject: text(row, 'subject'), text: text(row, 'text_body'), createdAt: text(row, 'created_at'), updatedAt: text(row, 'updated_at'),
  }));
  const scopeRows = all(db, 'SELECT token_id, scope FROM developer_token_scopes ORDER BY scope');
  const accountRows = all(db, 'SELECT token_id, account_id FROM developer_token_accounts ORDER BY account_id');
  const tokens = all(db, 'SELECT * FROM developer_tokens ORDER BY created_at DESC').map((row): DeveloperToken => {
    const token: DeveloperToken = {
      id: text(row, 'id'), name: text(row, 'name'), tokenHash: text(row, 'token_hash'), prefix: text(row, 'prefix'),
      scopes: scopeRows.filter((item) => text(item, 'token_id') === text(row, 'id')).map((item) => text(item, 'scope') as TokenScope),
      accountIds: accountRows.filter((item) => text(item, 'token_id') === text(row, 'id')).map((item) => text(item, 'account_id')),
      createdAt: text(row, 'created_at'), expiresAt: text(row, 'expires_at'),
    };
    const lastUsedAt = optionalText(row, 'last_used_at'); if (lastUsedAt) token.lastUsedAt = lastUsedAt;
    return token;
  });
  return { accounts, messages, tokens, drafts };
}
