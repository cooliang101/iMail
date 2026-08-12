import { isTauriRuntime } from './tauri-runtime';

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
