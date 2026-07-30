import type { DatabaseSync } from 'node:sqlite';
import type { DeveloperToken, Draft, LogoFetchAttempt, MailAccount, MailContact, StoreData, TokenScope } from '../types.js';
import { json, messageFromRow, optionalText, text, type Row } from './rows.js';

function all(db: DatabaseSync, sql: string, ...values: string[]) { return db.prepare(sql).all(...values) as Row[]; }
function owned<T extends { ownerId?: string }>(value: T, ownerId: string): T {
  Object.defineProperty(value, 'ownerId', { value: ownerId, enumerable: false, writable: true });
  return value;
}

export function readSnapshot(db: DatabaseSync, userId?: string): StoreData {
  const ownerWhere = userId ? ' WHERE user_id = ?' : '';
  const ownerValues = userId ? [userId] : [];
  const accounts = all(db, `SELECT * FROM accounts${ownerWhere} ORDER BY created_at`, ...ownerValues).map((row): MailAccount => {
    const account: MailAccount = {
      id: text(row, 'id'), provider: text(row, 'provider') as MailAccount['provider'], email: text(row, 'email'),
      displayName: text(row, 'display_name'), group: text(row, 'group_name'), color: text(row, 'color'),
      settings: json(row, 'settings_json'), encryptedSecret: text(row, 'encrypted_secret'),
      authMethod: optionalText(row, 'auth_method') as MailAccount['authMethod'], createdAt: text(row, 'created_at'),
      status: text(row, 'status') as MailAccount['status'],
    };
    const groupIcon = text(row, 'group_icon') as MailAccount['groupIcon']; if (groupIcon !== 'folder') account.groupIcon = groupIcon;
    const mailboxes = json<MailAccount['mailboxes']>(row, 'mailboxes_json'); if (mailboxes?.length) account.mailboxes = mailboxes;
    const lastSyncAt = optionalText(row, 'last_sync_at'); if (lastSyncAt) account.lastSyncAt = lastSyncAt;
    const lastError = optionalText(row, 'last_error'); if (lastError) account.lastError = lastError;
    return owned(account, text(row, 'user_id'));
  });
  const messages = all(db, userId ? 'SELECT m.* FROM messages m JOIN accounts a ON a.id = m.account_id WHERE a.user_id = ? ORDER BY m.received_at DESC' : 'SELECT * FROM messages ORDER BY received_at DESC', ...ownerValues).map(messageFromRow);
  const contacts = all(db, `SELECT * FROM contacts${ownerWhere} ORDER BY last_contact_at DESC, message_count DESC, address`, ...ownerValues).map((row): MailContact => {
    const contact: MailContact = {
      address: text(row, 'address'), name: text(row, 'name'), messageCount: Number(row.message_count), lastContactAt: text(row, 'last_contact_at'),
    };
    const logoKey = optionalText(row, 'logo_key');
    if (logoKey) contact.logo = {
      key: logoKey, contentType: text(row, 'logo_content_type'), sourceUrl: text(row, 'logo_source_url'), fetchedAt: text(row, 'logo_fetched_at'),
    };
    return owned(contact, text(row, 'user_id'));
  });
  const logoFetchAttempts = all(db, `SELECT * FROM logo_fetch_attempts${ownerWhere} ORDER BY attempted_at DESC`, ...ownerValues).map((row): LogoFetchAttempt => owned<LogoFetchAttempt>({
    target: text(row, 'target'), domainKey: text(row, 'domain_key'), status: text(row, 'status') as LogoFetchAttempt['status'],
    detail: text(row, 'detail'), attemptedAt: text(row, 'attempted_at'),
  }, text(row, 'user_id')));
  const drafts = all(db, userId ? 'SELECT d.* FROM drafts d JOIN accounts a ON a.id = d.account_id WHERE a.user_id = ? ORDER BY d.updated_at DESC' : 'SELECT * FROM drafts ORDER BY updated_at DESC', ...ownerValues).map((row): Draft => ({
    id: text(row, 'id'), accountId: text(row, 'account_id'), to: json(row, 'to_json'), cc: json(row, 'cc_json'),
    subject: text(row, 'subject'), text: text(row, 'text_body'), html: text(row, 'html_body'), attachments: json(row, 'attachments_json'), createdAt: text(row, 'created_at'), updatedAt: text(row, 'updated_at'),
  }));
  const scopeRows = all(db, 'SELECT token_id, scope FROM developer_token_scopes ORDER BY scope');
  const accountRows = all(db, 'SELECT token_id, account_id FROM developer_token_accounts ORDER BY account_id');
  const tokens = all(db, `SELECT * FROM developer_tokens${ownerWhere} ORDER BY created_at DESC`, ...ownerValues).map((row): DeveloperToken => {
    const token: DeveloperToken = {
      id: text(row, 'id'), name: text(row, 'name'), tokenHash: text(row, 'token_hash'), prefix: text(row, 'prefix'),
      scopes: scopeRows.filter((item) => text(item, 'token_id') === text(row, 'id')).map((item) => text(item, 'scope') as TokenScope),
      accountIds: accountRows.filter((item) => text(item, 'token_id') === text(row, 'id')).map((item) => text(item, 'account_id')),
      createdAt: text(row, 'created_at'), expiresAt: text(row, 'expires_at'),
    };
    const lastUsedAt = optionalText(row, 'last_used_at'); if (lastUsedAt) token.lastUsedAt = lastUsedAt;
    return owned(token, text(row, 'user_id'));
  });
  return { accounts, messages, tokens, drafts, contacts, logoFetchAttempts };
}
