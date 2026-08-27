import type { AppPreferences } from '../../app-model';
import { defaultCustomTheme, defaultThemeId, normalizeCustomTheme, normalizeThemeId } from '../appearance';
import { defaultShortcutBindings } from '../shortcuts/shortcut-model';
import { normalizeLanguage } from '../i18n';

export const preferencesStorageKey = 'imail.preferences.v1';
export function preferencesStorageKeyFor(userId: string) { return `${preferencesStorageKey}:${userId}`; }
export type GatewayPreferences = AppPreferences;

export const defaultAppPreferences: AppPreferences = {
  language: 'zh-CN',
  theme: defaultThemeId,
  customTheme: { ...defaultCustomTheme },
  startupView: 'inbox',
  markReadOnOpen: true,
  defaultMessageView: 'source',
  notificationKinds: { unread: true, snooze: true, error: true },
  shortcutBindings: { ...defaultShortcutBindings },
};

export function loadAppPreferences(storage: Pick<Storage, 'getItem'> = localStorage, key = preferencesStorageKey): AppPreferences {
  try {
    const saved = JSON.parse(storage.getItem(key) ?? '{}') as Partial<AppPreferences>;
    return {
      language: normalizeLanguage(saved.language),
      theme: normalizeThemeId(saved.theme),
      customTheme: normalizeCustomTheme(saved.customTheme),
      startupView: saved.startupView === 'starred' ? 'starred' : 'inbox',
      markReadOnOpen: typeof saved.markReadOnOpen === 'boolean' ? saved.markReadOnOpen : true,
      defaultMessageView: saved.defaultMessageView === 'rendered' ? 'rendered' : 'source',
      notificationKinds: {
        unread: saved.notificationKinds?.unread !== false,
        snooze: saved.notificationKinds?.snooze !== false,
        error: saved.notificationKinds?.error !== false,
      },
      shortcutBindings: Object.fromEntries(Object.entries(defaultShortcutBindings).map(([key, fallback]) => [key, typeof saved.shortcutBindings?.[key as keyof typeof defaultShortcutBindings] === 'string' ? saved.shortcutBindings[key as keyof typeof defaultShortcutBindings] : fallback])) as AppPreferences['shortcutBindings'],
    };
  } catch {
    return { ...defaultAppPreferences, customTheme: { ...defaultCustomTheme }, notificationKinds: { ...defaultAppPreferences.notificationKinds }, shortcutBindings: { ...defaultShortcutBindings } };
  }
}

export function saveAppPreferences(preferences: AppPreferences, storage: Pick<Storage, 'setItem'> = localStorage) {
  storage.setItem(preferencesStorageKey, JSON.stringify(preferences));
}

export function gatewayPreferencesPayload(preferences: AppPreferences): GatewayPreferences {
  return { ...preferences, customTheme: normalizeCustomTheme(preferences.customTheme) };
}

export function mergeGatewayPreferences(local: AppPreferences, remote: GatewayPreferences): AppPreferences {
  const remoteCustomTheme = normalizeCustomTheme(remote.customTheme);
  const migrateLocalCustomTheme = local.theme === 'custom'
    && remote.theme !== 'custom'
    && JSON.stringify(remoteCustomTheme) === JSON.stringify(defaultCustomTheme);
  return {
    ...remote,
    theme: migrateLocalCustomTheme ? 'custom' : normalizeThemeId(remote.theme),
    customTheme: migrateLocalCustomTheme ? normalizeCustomTheme(local.customTheme) : remoteCustomTheme,
  };
}
