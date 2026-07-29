export type ProviderId = 'outlook' | 'gmail' | 'qq' | 'yahoo' | 'hotmail' | 'icloud' | 'custom';
export type MailboxRole = 'inbox' | 'sent' | 'archive' | 'trash' | 'custom';
export type WorkspaceIconId = 'folder' | 'briefcase' | 'building' | 'home' | 'users' | 'code' | 'heart' | 'star';

export type MailboxFolder = {
  path: string;
  name: string;
  delimiter: string;
  specialUse?: string;
  selectable: boolean;
  subscribed: boolean;
  total?: number;
  unread?: number;
};

export type Account = {
  id: string;
  provider: ProviderId;
  email: string;
  displayName: string;
  group: string;
  groupIcon: WorkspaceIconId;
  color: string;
  status: 'connected' | 'syncing' | 'error';
  authMethod?: 'app-password' | 'oauth2';
  lastSyncAt?: string;
  lastError?: string;
  mailboxes: MailboxFolder[];
};

export type Message = {
  id: string;
  accountId: string;
  mailbox: string;
  mailboxRole: MailboxRole;
  from: { name: string; address: string };
  to: Array<{ name: string; address: string }>;
  subject: string;
  preview: string;
  text?: string;
  html?: string;
  date: string;
  unread: boolean;
  flagged: boolean;
  hasAttachments: boolean;
  attachments: Array<{ filename: string; contentType: string; size: number; index: number }>;
  labels: string[];
  snoozedUntil?: string;
};

export type Contact = {
  address: string;
  name: string;
  messageCount: number;
  lastContactAt: string;
};

export type Draft = {
  id: string;
  accountId: string;
  to: string[];
  cc: string[];
  subject: string;
  text: string;
  html: string;
  attachments: DraftAttachment[];
  createdAt: string;
  updatedAt: string;
};

export type DraftAttachment = {
  id: string;
  filename: string;
  contentType: string;
  size: number;
  data: string;
};

export type DeveloperToken = {
  id: string;
  name: string;
  prefix: string;
  scopes: string[];
  mailboxes: string[];
  createdAt: string;
  expiresAt: string;
  lastUsedAt?: string;
};
