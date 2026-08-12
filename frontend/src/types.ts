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

export type SyncFolderMode = 'inbox' | 'standard' | 'selected';
export type SyncPolicy = {
  accountId: string;
  enabled: boolean;
  folderMode: SyncFolderMode;
  selectedMailboxes: string[];
  notifyOnError: boolean;
  updatedAt: string;
};

export type MailboxSyncState = {
  accountId: string;
  mailbox: string;
  mailboxRole: MailboxRole;
  uidValidity?: string;
  lastSeenUid: number;
  lastAttemptAt?: string;
  lastSuccessAt?: string;
  nextSyncAt?: string;
  consecutiveFailures: number;
  connectionStatus: 'connected' | 'unreachable' | 'authRequired';
  syncState: 'idle' | 'scheduled' | 'running' | 'backoff' | 'paused';
  lastErrorCode?: string;
  lastErrorMessage?: string;
};

export type SyncJob = {
  id: string;
  accountId: string;
  mailbox?: string;
  mailboxRole: MailboxRole;
  reason: 'scheduled' | 'startup' | 'manual' | 'recovery';
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';
  createdAt: string;
  startedAt?: string;
  finishedAt?: string;
  syncedCount?: number;
  newCount?: number;
  updatedCount?: number;
  deletedCount?: number;
  errorMessage?: string;
};

export type AccountSyncStatus = { accountId: string; policy: SyncPolicy; states: MailboxSyncState[]; jobs: SyncJob[] };
export type SyncWorkerHealth = {
  workers: Array<{ workerId: string; processId: number; hostName: string; startedAt: string; heartbeatAt: string }>;
  queuedJobs: number;
  oldestQueuedAt?: string;
};
