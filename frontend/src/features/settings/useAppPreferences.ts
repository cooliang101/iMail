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

export function useAppPreferences(userId: string, setNotice: Dispatch<SetStateAction<Notice>>) {
  const preferencesKey = preferencesStorageKeyFor(userId);
  const shortcutsKey = shortcutStorageKeyFor(userId);
  const [preferences, setPreferences] = useState<AppPreferences>(() => loadAppPreferences(localStorage, preferencesKey));
  const [shortcutBindings, setShortcutBindings] = useState<ShortcutBindings>(() => preferences.shortcutBindings);
  const saveQueue = useRef<Promise<void>>(Promise.resolve());

  const persistLocal = useCallback((next: AppPreferences) => {
    setPreferences(next);
    setShortcutBindings(next.shortcutBindings);
    localStorage.setItem(preferencesKey, JSON.stringify(next));
    localStorage.setItem(shortcutsKey, JSON.stringify(next.shortcutBindings));
  }, [preferencesKey, shortcutsKey]);

  useEffect(() => {
    let cancelled = false;
    void api<{ preferences: GatewayPreferences }>('/api/preferences').then((result) => {
      if (cancelled) return;
      const local = loadAppPreferences(localStorage, preferencesKey);
      persistLocal(mergeGatewayPreferences(local, result.preferences));
    }).catch(() => undefined);
    return () => { cancelled = true; };
  }, [persistLocal, preferencesKey]);

  const savePreferences = useCallback((next: AppPreferences) => {
    persistLocal(next);
    saveQueue.current = saveQueue.current.then(async () => {
      const result = await api<{ preferences: GatewayPreferences }>('/api/preferences', {
        method: 'PATCH',
        body: JSON.stringify(gatewayPreferencesPayload(next)),
      });
      persistLocal(mergeGatewayPreferences(next, result.preferences));
    }).catch((error) => {
      setNotice({ kind: 'error', text: error instanceof Error ? `设置已保存在本机，但服务端同步失败：${error.message}` : '设置已保存在本机，但服务端同步失败' });
    });
  }, [persistLocal, setNotice]);

  const saveShortcutBindings = useCallback((bindings: ShortcutBindings) => {
    savePreferences({ ...preferences, shortcutBindings: bindings });
    setNotice({ kind: 'success', text: '快捷键已保存' });
  }, [preferences, savePreferences, setNotice]);

  return { preferences, shortcutBindings, savePreferences, saveShortcutBindings };
}
