import type { DesktopHttpInvoker } from './desktop-http';

export type LocalServiceState = 'notInstalled' | 'stopped' | 'starting' | 'running' | 'error';
export type LocalServiceStatus = {
  installed: boolean;
  dataPresent: boolean;
  enabled: boolean;
  running: boolean;
  state: LocalServiceState;
  url: string;
  version?: string;
  instanceId?: string;
  error?: string;
  diagnostic?: {
    failures: number;
    reason: string;
    exitCode?: number;
    updatedAtEpochSeconds: number;
  };
};

async function tauriInvoke<T>(command: string, args?: Record<string, unknown>) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

export function desktopLocalServiceStatus(invoker: DesktopHttpInvoker = tauriInvoke) {
  return invoker<LocalServiceStatus>('local_service_status');
}

export function desktopEnableLocalService(invoker: DesktopHttpInvoker = tauriInvoke) {
  return invoker<LocalServiceStatus>('local_service_enable');
}

export function desktopEnableLocalServiceAtPort(port: number, invoker: DesktopHttpInvoker = tauriInvoke) {
  return invoker<LocalServiceStatus>('local_service_enable', { port });
}

export function desktopPauseLocalService(invoker: DesktopHttpInvoker = tauriInvoke) {
  return invoker<LocalServiceStatus>('local_service_pause');
}

export function desktopRemoveLocalService(invoker: DesktopHttpInvoker = tauriInvoke) {
  return invoker<LocalServiceStatus>('local_service_remove');
}

export function desktopOpenLocalServiceLogs(invoker: DesktopHttpInvoker = tauriInvoke) {
  return invoker<void>('local_service_open_logs');
}
