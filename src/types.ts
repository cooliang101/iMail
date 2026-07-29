export type ProviderId = 'outlook' | 'gmail' | 'qq' | 'yahoo' | 'hotmail' | 'icloud' | 'custom';
export type MailboxRole = 'inbox' | 'sent' | 'archive' | 'trash';

export type Account = {
  id: string;
  provider: ProviderId;
  email: string;
  displayName: string;
  group: string;
  color: string;
  status: 'connected' | 'syncing' | 'error';
  authMethod?: 'app-password' | 'oauth2';
  lastSyncAt?: string;
  lastError?: string;
};

export type Message = {
  id: string;
  accountId: string;
  mailboxRole: MailboxRole;
  from: { name: string; address: string };
  to: Array<{ name: string; address: string }>;
  subject: string;
  preview: string;
  text?: string;
  date: string;
  unread: boolean;
  flagged: boolean;
  hasAttachments: boolean;
  attachments: Array<{ filename: string; contentType: string; size: number; index: number }>;
  labels: string[];
  snoozedUntil?: string;
};

export type Draft = {
  id: string;
  accountId: string;
  to: string[];
  cc: string[];
  subject: string;
  text: string;
  createdAt: string;
  updatedAt: string;
};

export type DeveloperToken = {
  id: string;
  name: string;
  prefix: string;
  scopes: string[];
  accountIds: string[];
  createdAt: string;
  expiresAt: string;
  lastUsedAt?: string;
};
