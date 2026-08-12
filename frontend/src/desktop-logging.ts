import type { DesktopHttpInvoker } from './desktop-http';
import { isTauriRuntime } from './platform/tauri-runtime';

export type DesktopLogLevel = 'info' | 'warn' | 'error';

async function tauriInvoke<T>(command: string, args?: Record<string, unknown>) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

export function describeDesktopLogValue(value: unknown) {
  try {
    if (value instanceof Error) return value.stack || `${value.name}: ${value.message}`;
    if (typeof value === 'string') return value;
    if (typeof value === 'number' || typeof value === 'boolean' || typeof value === 'bigint') return String(value);
    if (value === null) return 'null';
    if (value === undefined) return 'undefined';
    if (typeof value === 'object') return `[${value.constructor?.name || 'object'}]`;
    return `[${typeof value}]`;
  } catch {
    return '[unavailable]';
  }
}

export function desktopLog(
  level: DesktopLogLevel,
  event: string,
  message?: string,
  desktop = isTauriRuntime(),
  invoker: DesktopHttpInvoker = tauriInvoke,
) {
  if (!desktop) return Promise.resolve();
  return invoker<void>('desktop_log', { level, event, message }).catch(() => undefined);
}

export function installDesktopLogging() {
  if (!isTauriRuntime()) return () => undefined;
  let forwarding = false;
  const originalError = console.error.bind(console);
  const originalWarn = console.warn.bind(console);
  const forward = (level: DesktopLogLevel, event: string, values: unknown[]) => {
    if (forwarding) return;
    forwarding = true;
    void desktopLog(level, event, values.map(describeDesktopLogValue).join(' '));
    forwarding = false;
  };
  const onError = (event: ErrorEvent) => forward('error', 'frontend.unhandled_error', [
    event.error instanceof Error ? event.error : event.message,
    event.filename ? `source=${event.filename}:${event.lineno}:${event.colno}` : '',
  ]);
  const onUnhandledRejection = (event: PromiseRejectionEvent) => forward('error', 'frontend.unhandled_rejection', [event.reason]);

  console.error = (...values: unknown[]) => { originalError(...values); forward('error', 'frontend.console_error', values); };
  console.warn = (...values: unknown[]) => { originalWarn(...values); forward('warn', 'frontend.console_warn', values); };
  window.addEventListener('error', onError);
  window.addEventListener('unhandledrejection', onUnhandledRejection);
  void desktopLog('info', 'frontend.bootstrap', 'frontend logging and global error handlers installed');

  return () => {
    console.error = originalError;
    console.warn = originalWarn;
    window.removeEventListener('error', onError);
    window.removeEventListener('unhandledrejection', onUnhandledRejection);
  };
}
