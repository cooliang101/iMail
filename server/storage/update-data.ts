import type { DatabaseSync, StatementSync } from 'node:sqlite';
import { reconcileContacts } from '../contact-model.js';
import type { StoreData } from '../types.js';

function changed<T extends object>(before: T | undefined, after: T) {
  return !before || JSON.stringify(before) !== JSON.stringify(after);
}

function by<T extends object>(items: T[], key: keyof T) {
  return new Map(items.map((item) => [String(item[key]), item]));
}

function runForMissing<T extends object>(statement: StatementSync, before: T[], after: T[], key: keyof T) {
  const retained = new Set(after.map((item) => String(item[key])));
  for (const item of before) if (!retained.has(String(item[key]))) statement.run(String(item[key]));
}

export function updateData(db: DatabaseSync, before: StoreData, after: StoreData, userId: string) {
  const accountIds = new Set(after.accounts.map((account) => account.id));
  for (const message of after.messages) if (!accountIds.has(message.accountId)) throw new Error('邮件引用了不存在的邮箱账户');
  for (const draft of after.drafts ?? []) if (!accountIds.has(draft.accountId)) throw new Error('草稿引用了不存在的发件邮箱');
  for (const token of after.tokens) if (token.accountIds.some((accountId) => !accountIds.has(accountId))) throw new Error('授权码引用了不存在的邮箱账户');

  if (JSON.stringify(before.accounts) !== JSON.stringify(after.accounts)
    || JSON.stringify(before.messages) !== JSON.stringify(after.messages)
    || JSON.stringify(before.contacts ?? []) !== JSON.stringify(after.contacts ?? [])) {
    after.contacts = reconcileContacts(after);
  }
  const contacts = after.contacts ??= [];
  const logoFetchAttempts = after.logoFetchAttempts ??= [];
  const drafts = after.drafts ??= [];

  const beforeAccounts = by(before.accounts, 'id');
  const beforeMessages = by(before.messages, 'id');
  const beforeDrafts = by(before.drafts ?? [], 'id');
  const beforeContacts = by(before.contacts ?? [], 'address');
  const beforeAttempts = by(before.logoFetchAttempts ?? [], 'target');
  const beforeTokens = by(before.tokens, 'id');

  const upsertAccount = db.prepare(`INSERT INTO accounts
    (id, provider, email, display_name, group_name, group_icon, color, settings_json, encrypted_secret, auth_method, created_at, last_sync_at, status, last_error, mailboxes_json, user_id)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET provider=excluded.provider, email=excluded.email, display_name=excluded.display_name,
      group_name=excluded.group_name, group_icon=excluded.group_icon, color=excluded.color, settings_json=excluded.settings_json,
      encrypted_secret=excluded.encrypted_secret, auth_method=excluded.auth_method, last_sync_at=excluded.last_sync_at,
      status=excluded.status, last_error=excluded.last_error, mailboxes_json=excluded.mailboxes_json
    WHERE accounts.user_id=excluded.user_id`);
  const upsertMessage = db.prepare(`INSERT INTO messages
    (id, account_id, mailbox, mailbox_role, uid, message_id, from_json, to_json, subject, preview, text_body, html_body, received_at, unread, flagged, has_attachments, attachments_json, labels_json, snoozed_until)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET mailbox=excluded.mailbox, mailbox_role=excluded.mailbox_role, uid=excluded.uid,
      message_id=excluded.message_id, from_json=excluded.from_json, to_json=excluded.to_json, subject=excluded.subject,
      preview=excluded.preview, text_body=excluded.text_body, html_body=excluded.html_body, received_at=excluded.received_at,
      unread=excluded.unread, flagged=excluded.flagged, has_attachments=excluded.has_attachments,
      attachments_json=excluded.attachments_json, labels_json=excluded.labels_json, snoozed_until=excluded.snoozed_until`);
  const upsertDraft = db.prepare(`INSERT INTO drafts
    (id, account_id, to_json, cc_json, subject, text_body, html_body, attachments_json, created_at, updated_at)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET account_id=excluded.account_id, to_json=excluded.to_json, cc_json=excluded.cc_json,
      subject=excluded.subject, text_body=excluded.text_body, html_body=excluded.html_body,
      attachments_json=excluded.attachments_json, updated_at=excluded.updated_at`);
  const upsertContact = db.prepare(`INSERT INTO contacts
    (address, name, message_count, last_contact_at, logo_key, logo_content_type, logo_source_url, logo_fetched_at, user_id)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(user_id, address) DO UPDATE SET name=excluded.name, message_count=excluded.message_count,
      last_contact_at=excluded.last_contact_at, logo_key=excluded.logo_key, logo_content_type=excluded.logo_content_type,
      logo_source_url=excluded.logo_source_url, logo_fetched_at=excluded.logo_fetched_at`);
  const upsertAttempt = db.prepare(`INSERT INTO logo_fetch_attempts
    (target, domain_key, status, detail, attempted_at, user_id) VALUES (?, ?, ?, ?, ?, ?)
    ON CONFLICT(user_id, target) DO UPDATE SET domain_key=excluded.domain_key, status=excluded.status,
      detail=excluded.detail, attempted_at=excluded.attempted_at`);
  const upsertToken = db.prepare(`INSERT INTO developer_tokens
    (id, name, token_hash, prefix, created_at, expires_at, last_used_at, user_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET name=excluded.name, token_hash=excluded.token_hash, prefix=excluded.prefix,
      expires_at=excluded.expires_at, last_used_at=excluded.last_used_at WHERE developer_tokens.user_id=excluded.user_id`);

  const remainingTokenIds = new Set(after.tokens.map((token) => token.id));
  for (const token of before.tokens) if (!remainingTokenIds.has(token.id)) db.prepare('DELETE FROM developer_tokens WHERE id = ? AND user_id = ?').run(token.id, userId);
  runForMissing(db.prepare('DELETE FROM drafts WHERE id = ?'), before.drafts ?? [], drafts, 'id');
  runForMissing(db.prepare('DELETE FROM messages WHERE id = ?'), before.messages, after.messages, 'id');
  const remainingAccountIds = new Set(after.accounts.map((account) => account.id));
  for (const account of before.accounts) if (!remainingAccountIds.has(account.id)) db.prepare('DELETE FROM accounts WHERE id = ? AND user_id = ?').run(account.id, userId);
  const remainingContacts = new Set(contacts.map((contact) => contact.address.toLowerCase()));
  for (const contact of before.contacts ?? []) if (!remainingContacts.has(contact.address.toLowerCase())) db.prepare('DELETE FROM contacts WHERE user_id = ? AND address = ?').run(userId, contact.address);
  const remainingAttempts = new Set(logoFetchAttempts.map((attempt) => attempt.target));
  for (const attempt of before.logoFetchAttempts ?? []) if (!remainingAttempts.has(attempt.target)) db.prepare('DELETE FROM logo_fetch_attempts WHERE user_id = ? AND target = ?').run(userId, attempt.target);

  for (const account of after.accounts) if (changed(beforeAccounts.get(account.id), account)) upsertAccount.run(
    account.id, account.provider, account.email, account.displayName, account.group, account.groupIcon ?? 'folder', account.color,
    JSON.stringify(account.settings), account.encryptedSecret, account.authMethod ?? null, account.createdAt, account.lastSyncAt ?? null,
    account.status, account.lastError ?? null, JSON.stringify(account.mailboxes ?? []), userId,
  );
  for (const message of after.messages) if (changed(beforeMessages.get(message.id), message)) upsertMessage.run(
    message.id, message.accountId, message.mailbox, message.mailboxRole ?? 'inbox', message.uid, message.messageId ?? null,
    JSON.stringify(message.from), JSON.stringify(message.to), message.subject, message.preview, message.text, message.html ?? null,
    message.date, Number(message.unread), Number(message.flagged), Number(message.hasAttachments), JSON.stringify(message.attachments),
    JSON.stringify(message.labels ?? []), message.snoozedUntil ?? null,
  );
  for (const draft of drafts) if (changed(beforeDrafts.get(draft.id), draft)) upsertDraft.run(
    draft.id, draft.accountId, JSON.stringify(draft.to), JSON.stringify(draft.cc), draft.subject, draft.text, draft.html ?? '',
    JSON.stringify(draft.attachments ?? []), draft.createdAt, draft.updatedAt,
  );
  for (const contact of contacts) if (changed(beforeContacts.get(contact.address), contact)) upsertContact.run(
    contact.address, contact.name, contact.messageCount, contact.lastContactAt, contact.logo?.key ?? null,
    contact.logo?.contentType ?? null, contact.logo?.sourceUrl ?? null, contact.logo?.fetchedAt ?? null, userId,
  );
  for (const attempt of logoFetchAttempts) if (changed(beforeAttempts.get(attempt.target), attempt)) upsertAttempt.run(
    attempt.target, attempt.domainKey, attempt.status, attempt.detail, attempt.attemptedAt, userId,
  );
  for (const token of after.tokens) if (changed(beforeTokens.get(token.id), token)) {
    upsertToken.run(token.id, token.name, token.tokenHash, token.prefix, token.createdAt, token.expiresAt, token.lastUsedAt ?? null, userId);
    db.prepare('DELETE FROM developer_token_scopes WHERE token_id = ?').run(token.id);
    db.prepare('DELETE FROM developer_token_accounts WHERE token_id = ?').run(token.id);
    const insertScope = db.prepare('INSERT INTO developer_token_scopes (token_id, scope) VALUES (?, ?)');
    const insertAccount = db.prepare('INSERT INTO developer_token_accounts (token_id, account_id) VALUES (?, ?)');
    for (const scope of token.scopes) insertScope.run(token.id, scope);
    for (const accountId of token.accountIds) insertAccount.run(token.id, accountId);
  }
}
