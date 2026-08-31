import type { DeveloperToken } from './types';

export type Notice = { kind: 'success' | 'error'; text: string } | null;
export type AppView = 'inbox' | 'starred' | 'sent' | 'snoozed' | 'archive' | 'trash' | 'junk' | 'folder' | 'drafts' | 'contacts' | 'tokens' | 'search';

export type SearchFilters = {
  accountIds?: string[]; group?: string | null; q?: string | null; subject?: string | null;
  body?: string | null; sender?: string | null; recipient?: string | null;
  since?: string | null; before?: string | null; unread?: boolean | null; flagged?: boolean | null;
  hasAttachments?: boolean | null; labels?: string[]; mailboxRole?: string | null;
  mailbox?: string | null; mailboxName?: string | null; snoozed?: boolean | null;
};
export type SmartFolder = { id: string; name: string; filters: SearchFilters; createdAt: string; updatedAt: string };

export type ApiGatewayCredential = {
  rawToken: string;
  detail: DeveloperToken;
};

export type AppleHmeAddress = {
  anonymousId: string;
  email: string;
  label: string;
  note: string;
  forwardToEmail: string;
  active: boolean;
  origin: string;
  createdAt?: string;
};

export type MessageStats = {
  total: number;
  unread: number;
  byAccount: Array<{ accountId: string; total: number; unread: number }>;
  byGroup: Array<{ group: string; total: number; unread: number }>;
};

export type MessageBodyView = 'source' | 'rendered';
export type MailParticipant = { name: string; address: string };
export type ParticipantRole = 'sender' | 'recipient';
export type ParticipantFilters = { sender: MailParticipant | null; recipient: MailParticipant | null };
export type StartupView = 'inbox' | 'starred';
export type AppLanguage = 'zh-CN' | 'en-US';
export type AppThemeId = 'mint-fresh' | 'tech' | 'business-blue' | 'soft-neubrutalism' | 'constructivist-red' | 'custom';
export type CustomThemeRadius = 'compact' | 'balanced' | 'rounded';
export type CustomThemeShadow = 'none' | 'soft' | 'offset';
export type CustomThemeTypography = 'system' | 'technical' | 'rounded';

export type CustomThemeDefinition = {
  name: string;
  canvas: string;
  surface: string;
  surfaceSubtle: string;
  rail: string;
  text: string;
  textSecondary: string;
  border: string;
  accent: string;
  accentSubtle: string;
  radius: CustomThemeRadius;
  shadow: CustomThemeShadow;
  typography: CustomThemeTypography;
};
export type NotificationKind = MailNotification['kind'];

export type AppPreferences = {
  composition: CompositionPreferences;
  language: AppLanguage;
  theme: AppThemeId;
  customTheme: CustomThemeDefinition;
  startupView: StartupView;
  markReadOnOpen: boolean;
  defaultMessageView: MessageBodyView;
  notificationKinds: Record<NotificationKind, boolean>;
  shortcutBindings: ShortcutBindings;
};

export type AccountSignature = { accountId: string; text: string; newMessages: boolean; replies: boolean };
export type ComposeTemplate = { id: string; name: string; subject: string; text: string };
export type CompositionPreferences = { signatures: AccountSignature[]; templates: ComposeTemplate[] };
export type ComposeMode = 'new' | 'reply' | 'replyAll' | 'forward';

export type ShortcutActionId = 'focusSearch' | 'compose' | 'sync' | 'nextMessage' | 'previousMessage' | 'reply' | 'forward' | 'toggleStar' | 'markUnread' | 'archive' | 'delete' | 'openShortcutSettings';
export type ShortcutBindings = Record<ShortcutActionId, string>;

export type WorkspaceFolder = {
  group: string;
  name: string;
  unread: number;
  targets: Array<{ accountId: string; accountName: string; path: string }>;
};

export type ContextTarget =
  | { kind: 'message'; x: number; y: number; messageId: string }
  | { kind: 'account'; x: number; y: number; accountId: string }
  | { kind: 'workspace'; x: number; y: number; group: string }
  | { kind: 'folder'; x: number; y: number; folder: WorkspaceFolder }
  | { kind: 'background'; x: number; y: number };

export type MailNotification = {
  id: string;
  kind: 'error' | 'snooze' | 'unread';
  title: string;
  detail: string;
  date: string;
  messageId?: string;
  accountId: string;
};
