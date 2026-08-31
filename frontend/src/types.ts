export type ProviderId = 'outlook' | 'gmail' | 'qq' | 'yahoo' | 'hotmail' | 'icloud' | 'custom';
export type MailboxRole = 'inbox' | 'sent' | 'archive' | 'drafts' | 'trash' | 'junk' | 'custom';
export type WorkspaceIconId = 'folder' | 'briefcase' | 'building' | 'home' | 'users' | 'code' | 'heart' | 'star';

export type ServiceInfo = {
  service: 'imail';
  instanceId: string;
  version: string;
  protocolVersion: number;
  capabilities: {
    gateway: boolean;
    mcp: boolean;
    syncWorker: boolean;
    webClient: boolean;
  };
};

export type ExternalAccessSettings = {
  gatewayEnabled: boolean;
  mcpEnabled: boolean;
};

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

export type ProxyProtocol = 'http' | 'https' | 'socks5';
export type MailProxySettings = { protocol: ProxyProtocol; host: string; port: number; username?: string };

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
  proxy?: MailProxySettings;
};

export type Message = {
  messageId?: string;
  cc?: Array<{ name: string; address: string }>;
  replyTo?: Array<{ name: string; address: string }>;
  inReplyTo?: string[];
  references?: string[];
  id: string;
  accountId: string;
  mailbox: string;
  mailboxRole: MailboxRole;
  from: { name: string; address: string; logo: ContactLogo };
  to: Array<{ name: string; address: string }>;
  subject: string;
  preview: string;
  text?: string;
  html?: string;
  date: string;
  unread: boolean;
  flagged: boolean;
  hasAttachments: boolean;
  attachments: MessageAttachment[];
  labels: string[];
  snoozedUntil?: string;
};

export type MessageAttachment = {
  filename: string;
  contentType: string;
  size: number;
  index: number;
};

export type Contact = {
  address: string;
  name: string;
  messageCount: number;
  lastContactAt: string;
  logo: ContactLogo;
};

export type ContactLogo = {
  url: string;
  key?: string;
  contentType?: string;
  sourceUrl?: string;
  fetchedAt?: string;
};

export type Draft = {
  bcc?: string[];
  inReplyTo?: string[];
  references?: string[];
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
