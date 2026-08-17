import { isTauriRuntime } from '../../platform/tauri-runtime';
import { configuredServiceMode, embeddedTauriServiceEnabled } from '../config';
import { desktopHttpRequest, type DesktopHttpInvoker, type DesktopHttpResponse } from '../desktop/http';
import { HttpMailService } from './http';
import { TauriMailService } from './tauri';

export type { MailService, MailServiceKind } from './contracts';
export { HttpMailService } from './http';
export { embeddedDomainCall, TauriMailService } from './tauri';
export { embeddedTauriServiceEnabled } from '../config';

async function tauriInvoke<T>(command: string, args?: Record<string, unknown>) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

export function createMailService(options: {
  tauri?: boolean;
  mode?: 'local' | 'remote';
  embedded?: boolean;
  invoker?: DesktopHttpInvoker;
  httpRequester?: (path: string, options?: RequestInit) => Promise<DesktopHttpResponse>;
} = {}) {
  const tauri = options.tauri ?? isTauriRuntime();
  const mode = options.mode ?? configuredServiceMode();
  const embedded = options.embedded ?? embeddedTauriServiceEnabled();
  const invoker = options.invoker ?? tauriInvoke;
  if (tauri && mode === 'local' && embedded) return new TauriMailService(invoker);
  return new HttpMailService(options.httpRequester ?? ((path, request) => desktopHttpRequest(path, request)));
}
