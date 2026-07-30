import type { MailAccount, MailboxRole } from '../types.js';

export function mailboxRoleFor(identity?: { path?: string; specialUse?: string }) : MailboxRole {
  if (!identity || identity.path?.toUpperCase() === 'INBOX' || identity.specialUse === '\\Inbox') return 'inbox';
  if (identity.specialUse === '\\Sent') return 'sent';
  if (identity.specialUse === '\\Archive' || identity.specialUse === '\\All') return 'archive';
  if (identity.specialUse === '\\Trash') return 'trash';
  return 'custom';
}

export function canonicalSyncTarget(account: Pick<MailAccount, 'mailboxes'>, mailboxRole: MailboxRole = 'inbox', mailbox?: string) {
  if (!mailbox) return { mailboxRole };
  const folder = account.mailboxes?.find((item) => item.path === mailbox || (mailbox.toUpperCase() === 'INBOX' && item.path.toUpperCase() === 'INBOX'));
  const resolvedRole = mailboxRoleFor({ path: mailbox, specialUse: folder?.specialUse });
  return resolvedRole === 'custom' ? { mailboxRole: resolvedRole, mailbox } : { mailboxRole: resolvedRole };
}
