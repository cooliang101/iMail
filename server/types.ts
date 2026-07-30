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

export type MailSettings = {
  imapHost: string;
  imapPort: number;
  imapSecure: boolean;
  smtpHost: string;
  smtpPort: number;
  smtpSecure: boolean;
};

export type AccountSecret = {
  authType?: 'app-password' | 'oauth2';
  password?: string;
  accessToken?: string;
  refreshToken?: string;
  expiresAt?: string;
  oauthProvider?: 'google' | 'microsoft' | 'yahoo';
  scopes?: string[];
  tokenType?: string;
};

export type MailAccount = {
  id: string;
  provider: ProviderId;
  email: string;
  displayName: string;
  group: string;
  groupIcon?: WorkspaceIconId;
  color: string;
  settings: MailSettings;
  encryptedSecret: string;
  authMethod?: 'app-password' | 'oauth2';
  createdAt: string;
  lastSyncAt?: string;
  status: 'connected' | 'error' | 'syncing';
  lastError?: string;
  mailboxes?: MailboxFolder[];
};

export type CachedMessage = {
  id: string;
  accountId: string;
  mailbox: string;
  mailboxRole?: MailboxRole;
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
  attachments: Array<{ filename: string; contentType: string; size: number; index?: number }>;
  labels?: string[];
  snoozedUntil?: string;
};

export type ContactLogo = {
  key: string;
  contentType: string;
  sourceUrl: string;
  fetchedAt: string;
};

export type MailContact = {
  address: string;
  name: string;
  messageCount: number;
  lastContactAt: string;
  logo?: ContactLogo;
};

export type LogoFetchAttempt = {
  target: string;
  domainKey: string;
  status: 'success' | 'failed';
  detail: string;
  attemptedAt: string;
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

export type TokenScope = 'messages:read' | 'messages:send' | 'accounts:read' | 'mcp:full';

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
  drafts?: Draft[];
  contacts?: MailContact[];
  logoFetchAttempts?: LogoFetchAttempt[];
};
