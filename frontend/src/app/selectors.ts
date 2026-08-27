import type { Account, MailboxRole } from '../types';
import type { AppView, ParticipantFilters, WorkspaceFolder } from '../app-model';
import { isWorkspaceMailbox } from '../features/organize';

export function buildWorkspaceFolders(accounts: Account[], groups: string[]) {
  return new Map(groups.map((group) => {
    const merged = new Map<string, WorkspaceFolder>();
    for (const account of accounts.filter((item) => item.group === group)) {
      for (const mailbox of account.mailboxes.filter(isWorkspaceMailbox)) {
        const key = mailbox.name.toLocaleLowerCase();
        const current = merged.get(key) ?? { group, name: mailbox.name, unread: 0, targets: [] };
        current.unread += mailbox.unread ?? 0;
        current.targets.push({ accountId: account.id, accountName: account.displayName, path: mailbox.path });
        merged.set(key, current);
      }
    }
    return [group, Array.from(merged.values()).sort((left, right) => left.name.localeCompare(right.name, 'zh-CN'))] as const;
  }));
}

export function buildMessageQuery({ accountFilter, groupFilter, search, view, mailFilter, activeLabel, activeMailbox, participantFilters }: {
  accountFilter: string; groupFilter: string | null; search: string; view: AppView; mailFilter: 'all' | 'unread' | 'attachments'; activeLabel: string | null; activeMailbox: WorkspaceFolder | null; participantFilters?: ParticipantFilters;
}) {
  const params = new URLSearchParams();
  if (accountFilter !== 'all') params.set('accountId', accountFilter);
  if (groupFilter) params.set('group', groupFilter);
  if (search.trim()) params.set('q', search.trim());
  if (participantFilters?.sender?.address.trim()) params.set('sender', participantFilters.sender.address.trim());
  if (participantFilters?.recipient?.address.trim()) params.set('recipient', participantFilters.recipient.address.trim());
  if (view === 'starred') params.set('flagged', 'true');
  if (view === 'folder' && activeMailbox) { params.set('group', activeMailbox.group); params.set('mailboxName', activeMailbox.name); }
  else {
    const mailboxRole: MailboxRole = view === 'sent' ? 'sent'
      : view === 'archive' ? 'archive'
        : view === 'drafts' ? 'drafts'
          : view === 'trash' ? 'trash'
            : view === 'junk' ? 'junk'
              : 'inbox';
    params.set('mailboxRole', mailboxRole);
  }
  if (view === 'snoozed') params.set('snoozed', 'true');
  if (activeLabel) params.set('label', activeLabel);
  if (mailFilter === 'unread') params.set('unread', 'true');
  if (mailFilter === 'attachments') params.set('hasAttachments', 'true');
  return params.toString();
}
