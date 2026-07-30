export type Notice = { kind: 'success' | 'error'; text: string } | null;
export type AppView = 'inbox' | 'starred' | 'sent' | 'snoozed' | 'archive' | 'trash' | 'junk' | 'folder' | 'drafts' | 'tokens';

export type MessageStats = {
  total: number;
  unread: number;
  byAccount: Array<{ accountId: string; total: number; unread: number }>;
  byGroup: Array<{ group: string; total: number; unread: number }>;
};

export type MessageBodyView = 'source' | 'rendered';
export type StartupView = 'inbox' | 'starred';
export type AppThemeId = 'mint-fresh' | 'tech' | 'business-blue' | 'soft-neubrutalism' | 'custom';
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
  theme: AppThemeId;
  customTheme: CustomThemeDefinition;
  startupView: StartupView;
  markReadOnOpen: boolean;
  defaultMessageView: MessageBodyView;
  notificationKinds: Record<NotificationKind, boolean>;
  shortcutBindings: ShortcutBindings;
};

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
