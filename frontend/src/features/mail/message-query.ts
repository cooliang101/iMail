import type { AppView, ParticipantFilters, SearchFilters, WorkspaceFolder } from '../../app-model';
import type { MailboxRole } from '../../types';

export function buildMessageQuery({ accountFilter, groupFilter, search, view, mailFilter, activeLabel, activeMailbox, participantFilters, searchFilters }: {
  searchFilters?: SearchFilters | null;
  accountFilter: string; groupFilter: string | null; search: string; view: AppView; mailFilter: 'all' | 'unread' | 'attachments'; activeLabel: string | null; activeMailbox: WorkspaceFolder | null; participantFilters?: ParticipantFilters;
}) {
  const params = new URLSearchParams();
  if (view === 'search') {
    params.set('filters', JSON.stringify({ ...searchFilters, q: search.trim() || undefined }));
    if (mailFilter === 'unread') params.set('unread', 'true');
    if (mailFilter === 'attachments') params.set('hasAttachments', 'true');
    if (participantFilters?.sender) params.set('sender', participantFilters.sender.address);
    if (participantFilters?.recipient) params.set('recipient', participantFilters.recipient.address);
    return params.toString();
  }
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
