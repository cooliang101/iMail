export type TauriWindow = Window & { __TAURI_INTERNALS__?: unknown };

export function isTauriRuntime(candidate?: Pick<TauriWindow, '__TAURI_INTERNALS__'>) {
  const runtime = candidate ?? (typeof window === 'undefined' ? {} : window as TauriWindow);
  return runtime.__TAURI_INTERNALS__ !== undefined;
}
