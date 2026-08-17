import type { DesktopHttpInvoker } from './http';

async function tauriInvoke<T>(command: string, args?: Record<string, unknown>) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

export function desktopOpenAppLogs(invoker: DesktopHttpInvoker = tauriInvoke) {
  return invoker<void>('desktop_open_app_logs');
}
