export type ProviderId = 'outlook' | 'gmail' | 'qq' | 'yahoo' | 'hotmail' | 'icloud' | 'custom';
export type MailboxRole = 'inbox' | 'sent' | 'archive' | 'drafts' | 'trash' | 'junk' | 'custom';
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
  ownerId?: string;
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

export type MailboxMessageChange = {
  before?: CachedMessage;
  after?: CachedMessage;
};

export type ContactLogo = {
  key: string;
  contentType: string;
  sourceUrl: string;
  fetchedAt: string;
};

export type MailContact = {
  ownerId?: string;
  address: string;
  name: string;
  messageCount: number;
  lastContactAt: string;
  logo?: ContactLogo;
};

export type LogoFetchAttempt = {
  ownerId?: string;
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
  ownerId?: string;
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

export type SyncFolderMode = 'inbox' | 'standard' | 'selected';
export type SyncJobReason = 'scheduled' | 'startup' | 'manual' | 'recovery';
export type SyncJobStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';
export type SyncConnectionStatus = 'connected' | 'unreachable' | 'authRequired';
export type SyncState = 'idle' | 'scheduled' | 'running' | 'backoff' | 'paused';

export type SyncPolicy = {
  accountId: string;
  enabled: boolean;
  intervalMinutes: number;
  folderMode: SyncFolderMode;
  selectedMailboxes: string[];
  syncOnStart: boolean;
  retryOnRecovery: boolean;
  notifyOnError: boolean;
  updatedAt: string;
};

export type SyncPolicySettings = Omit<SyncPolicy, 'accountId' | 'updatedAt'>;

export type MailboxSyncState = {
  accountId: string;
  mailbox: string;
  mailboxRole: MailboxRole;
  uidValidity?: string;
  lastSeenUid: number;
  highestModseq?: string;
  lastAttemptAt?: string;
  lastSuccessAt?: string;
  nextSyncAt?: string;
  consecutiveFailures: number;
  connectionStatus: SyncConnectionStatus;
  syncState: SyncState;
  lastErrorCode?: string;
  lastErrorMessage?: string;
};

export type SyncJob = {
  id: string;
  accountId: string;
  mailbox?: string;
  mailboxRole: MailboxRole;
  reason: SyncJobReason;
  status: SyncJobStatus;
  priority: number;
  notBefore: string;
  lockedBy?: string;
  lockedUntil?: string;
  attempts: number;
  createdAt: string;
  startedAt?: string;
  finishedAt?: string;
  syncedCount?: number;
  newCount?: number;
  updatedCount?: number;
  deletedCount?: number;
  errorCode?: string;
  errorMessage?: string;
};

export type SyncEvent = {
  id: number;
  type: 'sync.started' | 'sync.completed' | 'sync.failed' | 'message.created';
  accountId: string;
  jobId?: string;
  payload: Record<string, unknown>;
  createdAt: string;
};
