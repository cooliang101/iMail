import type { CachedMessage, DeveloperToken, MailAccount } from '../types.js';

export function publicAccount(account: MailAccount) {
  const { encryptedSecret: _secret, ...safe } = account;
  return { ...safe, authMethod: safe.authMethod ?? 'app-password', groupIcon: safe.groupIcon ?? 'folder', mailboxes: safe.mailboxes ?? [] };
}

export function publicDeveloperToken(token: DeveloperToken, accounts: MailAccount[]) {
  const { tokenHash: _hash, accountIds, ...safe } = token;
  const emailsById = new Map(accounts.map((account) => [account.id, account.email]));
  const mailboxes = accountIds.map((id) => emailsById.get(id)).filter((email): email is string => Boolean(email));
  return { ...safe, mailboxes };
}

export function publicMessageSummary(message: CachedMessage) {
  const { uid: _uid, messageId: _messageId, text: _text, html: _html, ...summary } = message;
  return {
    ...summary,
    mailboxRole: summary.mailboxRole ?? 'inbox',
    attachments: summary.attachments.map((attachment, index) => ({ ...attachment, index: attachment.index ?? index })),
    labels: summary.labels ?? [],
    from: { ...summary.from, logo: { url: `/api/contacts/logo?address=${encodeURIComponent(summary.from.address)}` } },
  };
}
