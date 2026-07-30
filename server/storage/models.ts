import type { CachedMessage, MailboxFolder, MailboxRole } from '../types.js';

export type MessageQuery = {
  accountId?: string;
  group?: string;
  query?: string;
  unread?: boolean;
  flagged?: boolean;
  hasAttachments?: boolean;
  mailboxRole?: MailboxRole;
  mailbox?: string;
  mailboxName?: string;
  snoozed?: boolean;
  label?: string;
  limit: number;
  offset: number;
};

export type MessageStats = {
  total: number;
  unread: number;
  byAccount: Array<{ accountId: string; total: number; unread: number }>;
  byGroup: Array<{ group: string; total: number; unread: number }>;
};

export type MailboxSyncCommit = {
  accountId: string;
  mailbox: string;
  mailboxRole: MailboxRole;
  incoming: CachedMessage[];
  removedUids: number[];
  uidValidityChanged: boolean;
  flagUpdates: Array<{ uid: number; unread: boolean; flagged: boolean }>;
  folders: MailboxFolder[];
  completedAt: string;
};
