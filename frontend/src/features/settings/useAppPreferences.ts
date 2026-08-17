import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from 'react';
import { api } from '../../api';
import type { AppPreferences, Notice, ShortcutBindings } from '../../app-model';
import {
  gatewayPreferencesPayload,
  loadAppPreferences,
  mergeGatewayPreferences,
  preferencesStorageKeyFor,
  type GatewayPreferences,
} from './settings-model';
import { shortcutStorageKeyFor } from '../shortcuts';

export type PreferencesSyncIssue = {
  message: string;
};

export function useAppPreferences(userId: string, setNotice: Dispatch<SetStateAction<Notice>>) {
  const preferencesKey = preferencesStorageKeyFor(userId);
  const shortcutsKey = shortcutStorageKeyFor(userId);
  const [preferences, setPreferences] = useState<AppPreferences>(() => loadAppPreferences(localStorage, preferencesKey));
  const [shortcutBindings, setShortcutBindings] = useState<ShortcutBindings>(() => preferences.shortcutBindings);
  const [preferencesSyncIssue, setPreferencesSyncIssue] = useState<PreferencesSyncIssue | null>(null);
  const saveQueue = useRef<Promise<void>>(Promise.resolve());
  const localEditRevision = useRef(0);

  const persistLocal = useCallback((next: AppPreferences) => {
    setPreferences(next);
    setShortcutBindings(next.shortcutBindings);
    try {
      localStorage.setItem(preferencesKey, JSON.stringify(next));
      localStorage.setItem(shortcutsKey, JSON.stringify(next.shortcutBindings));
    } catch (error) {
      setPreferencesSyncIssue({ message: error instanceof Error ? `设置已应用，但本机存储失败：${error.message}` : '设置已应用，但本机存储失败。' });
    }
  }, [preferencesKey, shortcutsKey]);

  useEffect(() => {
    let cancelled = false;
    void api<{ preferences: GatewayPreferences }>('/api/preferences').then((result) => {
      if (cancelled || localEditRevision.current > 0) return;
      const local = loadAppPreferences(localStorage, preferencesKey);
      const merged = mergeGatewayPreferences(local, result.preferences);
      persistLocal(merged);
      if (merged.theme === 'custom' && result.preferences.theme !== 'custom') {
        saveQueue.current = saveQueue.current.then(async () => {
          await api<{ preferences: GatewayPreferences }>('/api/preferences', {
            method: 'PATCH',
            body: JSON.stringify(gatewayPreferencesPayload(merged)),
          });
        }).catch((error) => {
          if (!cancelled) setPreferencesSyncIssue({
            message: error instanceof Error
              ? `本机自定义主题迁移到服务端失败：${error.message}`
              : '本机自定义主题迁移到服务端失败。',
          });
        });
      }
    }).catch((error) => {
      if (!cancelled) setPreferencesSyncIssue({
        message: error instanceof Error
          ? `无法读取服务端设置，已继续使用本机缓存：${error.message}`
          : '无法读取服务端设置，已继续使用本机缓存。',
      });
    });
    return () => { cancelled = true; };
  }, [persistLocal, preferencesKey]);

  const savePreferences = useCallback((next: AppPreferences) => {
    localEditRevision.current += 1;
    persistLocal(next);
    saveQueue.current = saveQueue.current.then(async () => {
      await api<{ preferences: GatewayPreferences }>('/api/preferences', {
        method: 'PATCH',
        body: JSON.stringify(gatewayPreferencesPayload(next)),
      });
    }).catch((error) => {
      setPreferencesSyncIssue({
        message: error instanceof Error
          ? `设置已保存在本机，但服务端同步失败：${error.message}`
          : '设置已保存在本机，但服务端同步失败。',
      });
    });
  }, [persistLocal]);

  const dismissPreferencesSyncIssue = useCallback(() => {
    setPreferencesSyncIssue(null);
  }, []);

  const saveShortcutBindings = useCallback((bindings: ShortcutBindings) => {
    savePreferences({ ...preferences, shortcutBindings: bindings });
    setNotice({ kind: 'success', text: '快捷键已保存' });
  }, [preferences, savePreferences, setNotice]);

  return {
    preferences,
    shortcutBindings,
    preferencesSyncIssue,
    dismissPreferencesSyncIssue,
    savePreferences,
    saveShortcutBindings,
  };
}
