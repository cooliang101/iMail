import type { MailAccount, MailboxRole } from '../types.js';

const roleBySpecialUse: Partial<Record<string, MailboxRole>> = {
  '\\inbox': 'inbox',
  '\\sent': 'sent',
  '\\archive': 'archive',
  '\\all': 'archive',
  '\\drafts': 'drafts',
  '\\trash': 'trash',
  '\\junk': 'junk',
};

const roleByFolderName: Partial<Record<string, MailboxRole>> = {
  inbox: 'inbox',
  sent: 'sent',
  'sent items': 'sent',
  'sent messages': 'sent',
  'sent mail': 'sent',
  archive: 'archive',
  archives: 'archive',
  'all mail': 'archive',
  draft: 'drafts',
  drafts: 'drafts',
  trash: 'trash',
  bin: 'trash',
  deleted: 'trash',
  'deleted item': 'trash',
  'deleted items': 'trash',
  'deleted message': 'trash',
  'deleted messages': 'trash',
  junk: 'junk',
  spam: 'junk',
  'junk mail': 'junk',
  'junk email': 'junk',
  'bulk mail': 'junk',
};

function normalizedLeaf(path = '') {
  return path.split(/[/.\\]/).at(-1)?.trim().toLocaleLowerCase().replace(/\be[-_]?mail\b/g, 'email').replace(/[-_]+/g, ' ').replace(/\s+/g, ' ') ?? '';
}

export function mailboxRoleFor(identity?: { path?: string; specialUse?: string }) : MailboxRole {
  if (!identity) return 'inbox';
  const specialUseRole = roleBySpecialUse[identity.specialUse?.toLocaleLowerCase() ?? ''];
  return specialUseRole ?? roleByFolderName[normalizedLeaf(identity.path)] ?? 'custom';
}

export function canonicalSyncTarget(account: Pick<MailAccount, 'mailboxes'>, mailboxRole: MailboxRole = 'inbox', mailbox?: string) {
  if (!mailbox) return { mailboxRole };
  const folder = account.mailboxes?.find((item) => item.path === mailbox || (mailbox.toUpperCase() === 'INBOX' && item.path.toUpperCase() === 'INBOX'));
  const resolvedRole = mailboxRoleFor({ path: mailbox, specialUse: folder?.specialUse });
  return resolvedRole === 'custom' ? { mailboxRole: resolvedRole, mailbox } : { mailboxRole: resolvedRole };
}
