export type ProviderId = 'outlook' | 'gmail' | 'qq' | 'yahoo' | 'hotmail' | 'icloud' | 'custom';

export type MailSettings = {
  imapHost: string;
  imapPort: number;
  imapSecure: boolean;
  smtpHost: string;
  smtpPort: number;
  smtpSecure: boolean;
};

export type AccountSecret = {
  password?: string;
  accessToken?: string;
};

export type MailAccount = {
  id: string;
  provider: ProviderId;
  email: string;
  displayName: string;
  group: string;
  color: string;
  settings: MailSettings;
  encryptedSecret: string;
  createdAt: string;
  lastSyncAt?: string;
  status: 'connected' | 'error' | 'syncing';
  lastError?: string;
};

export type CachedMessage = {
  id: string;
  accountId: string;
  mailbox: string;
  uid: number;
  messageId?: string;
  from: { name: string; address: string };
  to: Array<{ name: string; address: string }>;
  subject: string;
  preview: string;
  text: string;
  html?: string;
  date: string;
  unread: boolean;
  flagged: boolean;
  hasAttachments: boolean;
  attachments: Array<{ filename: string; contentType: string; size: number }>;
};

export type TokenScope = 'messages:read' | 'messages:send' | 'accounts:read';

export type DeveloperToken = {
  id: string;
  name: string;
  tokenHash: string;
  prefix: string;
  scopes: TokenScope[];
  accountIds: string[];
  createdAt: string;
  expiresAt: string;
  lastUsedAt?: string;
};

export type StoreData = {
  accounts: MailAccount[];
  messages: CachedMessage[];
  tokens: DeveloperToken[];
};
