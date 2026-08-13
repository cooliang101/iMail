import { isTauriRuntime } from './tauri-runtime';
import type { CustomThemeDefinition, AppThemeId } from '../app-model';
import type { Account } from '../types';

export async function updateDesktopTrayMenu(accounts: Account[], themeId: AppThemeId, customTheme: CustomThemeDefinition) {
  if (!isTauriRuntime()) return;
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('desktop_update_tray_menu', {
    update: {
      accounts: accounts.map(({ id, email, displayName, provider, color }) => ({
        id, email, displayName, provider, color,
      })),
      themeId,
      customTheme,
    },
  });
}

export function subscribeDesktopCompose(listener: () => void) {
  if (!isTauriRuntime()) return () => undefined;
  let closed = false;
  let unlisten: (() => void) | undefined;
  void import('@tauri-apps/api/event').then(async ({ listen }) => {
    unlisten = await listen('desktop-compose', () => { if (!closed) listener(); });
    if (closed) unlisten();
  }).catch(() => undefined);
  return () => { closed = true; unlisten?.(); };
}

export function subscribeDesktopAccountSelection(listener: (accountId: string) => void) {
  if (!isTauriRuntime()) return () => undefined;
  let closed = false;
  let unlisten: (() => void) | undefined;
  void import('@tauri-apps/api/event').then(async ({ listen }) => {
    unlisten = await listen<string>('desktop-select-account', ({ payload }) => {
      if (!closed && payload) listener(payload);
    });
    if (closed) unlisten();
  }).catch(() => undefined);
  return () => { closed = true; unlisten?.(); };
}
