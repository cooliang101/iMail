import type { DatabaseSync } from 'node:sqlite';
import { reconcileContacts } from '../contact-model.js';
import type { StoreData } from '../types.js';

export function replaceData(db: DatabaseSync, data: StoreData, migratedAt?: string, manageTransaction = true, userId?: string) {
  const insertAccount = db.prepare(`INSERT INTO accounts
    (id, provider, email, display_name, group_name, group_icon, color, settings_json, encrypted_secret, auth_method, created_at, last_sync_at, status, last_error, mailboxes_json, user_id)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`);
  const insertMessage = db.prepare(`INSERT INTO messages
    (id, account_id, mailbox, mailbox_role, uid, message_id, from_json, to_json, subject, preview, text_body, html_body, received_at, unread, flagged, has_attachments, attachments_json, labels_json, snoozed_until)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`);
  const insertDraft = db.prepare('INSERT INTO drafts (id, account_id, to_json, cc_json, subject, text_body, html_body, attachments_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)');
  const insertContact = db.prepare('INSERT INTO contacts (address, name, message_count, last_contact_at, logo_key, logo_content_type, logo_source_url, logo_fetched_at, user_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)');
  const insertLogoAttempt = db.prepare('INSERT INTO logo_fetch_attempts (target, domain_key, status, detail, attempted_at, user_id) VALUES (?, ?, ?, ?, ?, ?)');
  const insertToken = db.prepare('INSERT INTO developer_tokens (id, name, token_hash, prefix, created_at, expires_at, last_used_at, user_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)');
  const insertScope = db.prepare('INSERT INTO developer_token_scopes (token_id, scope) VALUES (?, ?)');
  const insertTokenAccount = db.prepare('INSERT INTO developer_token_accounts (token_id, account_id) VALUES (?, ?)');
  if (manageTransaction) db.exec('BEGIN IMMEDIATE');
  try {
    if (userId) {
      db.prepare('DELETE FROM developer_token_accounts WHERE token_id IN (SELECT id FROM developer_tokens WHERE user_id = ?)').run(userId);
      db.prepare('DELETE FROM developer_token_scopes WHERE token_id IN (SELECT id FROM developer_tokens WHERE user_id = ?)').run(userId);
      db.prepare('DELETE FROM developer_tokens WHERE user_id = ?').run(userId);
      db.prepare('DELETE FROM drafts WHERE account_id IN (SELECT id FROM accounts WHERE user_id = ?)').run(userId);
      db.prepare('DELETE FROM contacts WHERE user_id = ?').run(userId);
      db.prepare('DELETE FROM logo_fetch_attempts WHERE user_id = ?').run(userId);
      db.prepare('DELETE FROM messages WHERE account_id IN (SELECT id FROM accounts WHERE user_id = ?)').run(userId);
      db.prepare('DELETE FROM accounts WHERE user_id = ?').run(userId);
    } else db.exec('DELETE FROM developer_token_accounts; DELETE FROM developer_token_scopes; DELETE FROM developer_tokens; DELETE FROM drafts; DELETE FROM contacts; DELETE FROM logo_fetch_attempts; DELETE FROM messages; DELETE FROM accounts;');
    for (const account of data.accounts) insertAccount.run(account.id, account.provider, account.email, account.displayName, account.group, account.groupIcon ?? 'folder', account.color, JSON.stringify(account.settings), account.encryptedSecret, account.authMethod ?? null, account.createdAt, account.lastSyncAt ?? null, account.status, account.lastError ?? null, JSON.stringify(account.mailboxes ?? []), userId ?? account.ownerId ?? '__legacy__');
    for (const message of data.messages) insertMessage.run(message.id, message.accountId, message.mailbox, message.mailboxRole ?? 'inbox', message.uid, message.messageId ?? null, JSON.stringify(message.from), JSON.stringify(message.to), message.subject, message.preview, message.text, message.html ?? null, message.date, Number(message.unread), Number(message.flagged), Number(message.hasAttachments), JSON.stringify(message.attachments), JSON.stringify(message.labels ?? []), message.snoozedUntil ?? null);
    const contactsWithOwners: Array<{ contact: NonNullable<StoreData['contacts']>[number]; ownerId: string }> = [];
    if (userId) {
      data.contacts = reconcileContacts(data);
      for (const contact of data.contacts) contactsWithOwners.push({ contact, ownerId: userId });
    } else {
      const owners = new Set(data.accounts.map((account) => account.ownerId ?? '__legacy__'));
      for (const ownerId of owners) {
        const ownerAccounts = data.accounts.filter((account) => (account.ownerId ?? '__legacy__') === ownerId);
        const accountIds = new Set(ownerAccounts.map((account) => account.id));
        const contacts = reconcileContacts({ accounts: ownerAccounts, messages: data.messages.filter((message) => accountIds.has(message.accountId)), contacts: (data.contacts ?? []).filter((contact) => (contact.ownerId ?? ownerId) === ownerId) });
        for (const contact of contacts) contactsWithOwners.push({ contact, ownerId });
      }
      data.contacts = contactsWithOwners.map(({ contact }) => contact);
    }
    for (const { contact, ownerId: contactOwner } of contactsWithOwners) insertContact.run(contact.address, contact.name, contact.messageCount, contact.lastContactAt, contact.logo?.key ?? null, contact.logo?.contentType ?? null, contact.logo?.sourceUrl ?? null, contact.logo?.fetchedAt ?? null, contactOwner);
    for (const attempt of data.logoFetchAttempts ?? []) insertLogoAttempt.run(attempt.target, attempt.domainKey, attempt.status, attempt.detail, attempt.attemptedAt, userId ?? attempt.ownerId ?? '__legacy__');
    for (const draft of data.drafts ?? []) insertDraft.run(draft.id, draft.accountId, JSON.stringify(draft.to), JSON.stringify(draft.cc), draft.subject, draft.text, draft.html ?? '', JSON.stringify(draft.attachments ?? []), draft.createdAt, draft.updatedAt);
    for (const token of data.tokens) {
      insertToken.run(token.id, token.name, token.tokenHash, token.prefix, token.createdAt, token.expiresAt, token.lastUsedAt ?? null, userId ?? token.ownerId ?? '__legacy__');
      for (const scope of token.scopes) insertScope.run(token.id, scope);
      for (const accountId of token.accountIds) insertTokenAccount.run(token.id, accountId);
    }
    if (migratedAt) db.prepare("INSERT OR REPLACE INTO metadata (key, value) VALUES ('legacy_json_migrated_at', ?)").run(migratedAt);
    if (manageTransaction) db.exec('COMMIT');
  } catch (error) {
    if (manageTransaction) db.exec('ROLLBACK');
    throw error;
  }
}
