import { z } from 'zod';
import { getMetadata, setMetadata } from './store.js';

const metadataKey = 'app_preferences_v1';

const shortcutBindingsSchema = z.object({
  focusSearch: z.string().max(60), compose: z.string().max(60), sync: z.string().max(60), nextMessage: z.string().max(60),
  previousMessage: z.string().max(60), reply: z.string().max(60), forward: z.string().max(60), toggleStar: z.string().max(60),
  markUnread: z.string().max(60), archive: z.string().max(60), delete: z.string().max(60), openShortcutSettings: z.string().max(60),
});

export const appPreferencesSchema = z.object({
  startupView: z.enum(['inbox', 'starred']),
  markReadOnOpen: z.boolean(),
  defaultMessageView: z.enum(['source', 'rendered']),
  notificationKinds: z.object({ unread: z.boolean(), snooze: z.boolean(), error: z.boolean() }),
  shortcutBindings: shortcutBindingsSchema,
});

export const appPreferencesUpdateSchema = appPreferencesSchema.partial().extend({
  notificationKinds: appPreferencesSchema.shape.notificationKinds.partial().optional(),
  shortcutBindings: shortcutBindingsSchema.partial().optional(),
}).refine((value) => Object.keys(value).length > 0, '至少提供一个要更新的设置');

export type AppPreferences = z.infer<typeof appPreferencesSchema>;

export const defaultAppPreferences: AppPreferences = {
  startupView: 'inbox', markReadOnOpen: true, defaultMessageView: 'source',
  notificationKinds: { unread: true, snooze: true, error: true },
  shortcutBindings: {
    focusSearch: 'Mod+K', compose: 'C', sync: '', nextMessage: 'J', previousMessage: 'K', reply: 'R', forward: 'F',
    toggleStar: 'S', markUnread: 'U', archive: 'E', delete: 'Shift+#', openShortcutSettings: 'Mod+/',
  },
};

export async function readAppPreferences(): Promise<AppPreferences> {
  const stored = await getMetadata(metadataKey);
  if (!stored) return structuredClone(defaultAppPreferences);
  try {
    const saved = appPreferencesSchema.partial().parse(JSON.parse(stored));
    return appPreferencesSchema.parse({
      ...defaultAppPreferences,
      ...saved,
      notificationKinds: { ...defaultAppPreferences.notificationKinds, ...saved.notificationKinds },
      shortcutBindings: { ...defaultAppPreferences.shortcutBindings, ...saved.shortcutBindings },
    });
  } catch {
    return structuredClone(defaultAppPreferences);
  }
}

export async function updateAppPreferences(changes: z.infer<typeof appPreferencesUpdateSchema>): Promise<AppPreferences> {
  const current = await readAppPreferences();
  const next = appPreferencesSchema.parse({
    ...current,
    ...changes,
    notificationKinds: { ...current.notificationKinds, ...changes.notificationKinds },
    shortcutBindings: { ...current.shortcutBindings, ...changes.shortcutBindings },
  });
  await setMetadata(metadataKey, JSON.stringify(next));
  return next;
}
