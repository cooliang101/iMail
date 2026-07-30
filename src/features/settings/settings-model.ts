import type { AppPreferences } from '../../app-model';
import { defaultShortcutBindings } from '../shortcuts/shortcut-model';

export const preferencesStorageKey = 'imail.preferences.v1';
export function preferencesStorageKeyFor(userId: string) { return `${preferencesStorageKey}:${userId}`; }

export const defaultAppPreferences: AppPreferences = {
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
    return { ...defaultAppPreferences, notificationKinds: { ...defaultAppPreferences.notificationKinds }, shortcutBindings: { ...defaultShortcutBindings } };
  }
}

export function saveAppPreferences(preferences: AppPreferences, storage: Pick<Storage, 'setItem'> = localStorage) {
  storage.setItem(preferencesStorageKey, JSON.stringify(preferences));
}
