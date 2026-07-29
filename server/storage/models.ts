import type { MailboxRole } from '../types.js';

export type MessageQuery = {
  accountId?: string;
  group?: string;
  query?: string;
  unread?: boolean;
  flagged?: boolean;
  hasAttachments?: boolean;
  mailboxRole?: MailboxRole;
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
