import { createContext, useContext, type ReactNode } from 'react';
import type { DesktopNotification, DownloadRequest, PlatformRuntime } from './types';
import { desktopDownload } from '../desktop-http';
import { isTauriRuntime } from './tauri-runtime';
export { isTauriRuntime } from './tauri-runtime';
export function externalHttpUrl(value: string) {
  const url = new URL(value);
  if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error('只允许打开 HTTP 或 HTTPS 地址');
  return url.href;
}

async function webDownload({ url, filename }: DownloadRequest) {
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  anchor.rel = 'noreferrer';
  anchor.click();
}

function createWebRuntime(): PlatformRuntime {
  return {
    kind: 'web',
    async openExternal(value) {
      const opened = window.open(externalHttpUrl(value), '_blank', 'noopener,noreferrer');
      if (!opened) throw new Error('浏览器阻止了新窗口，请允许弹出窗口后重试');
    },
    saveDownload: webDownload,
    async notify({ title, body }: DesktopNotification) {
      if (!('Notification' in window)) return;
      if (Notification.permission === 'granted') new Notification(title, { body });
    },
  };
}

function createTauriRuntime(): PlatformRuntime {
  return {
    kind: 'tauri',
    async openExternal(value) {
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      await openUrl(externalHttpUrl(value));
    },
    async saveDownload({ url, filename }) {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const target = await save({ defaultPath: filename });
      if (!target) return;
      await desktopDownload(url, target);
    },
    async notify(input) {
      const { isPermissionGranted, requestPermission, sendNotification } = await import('@tauri-apps/plugin-notification');
      const granted = await isPermissionGranted() || await requestPermission() === 'granted';
      if (granted) sendNotification(input);
    },
  };
}

export function createPlatformRuntime(kind = isTauriRuntime() ? 'tauri' : 'web'): PlatformRuntime {
  return kind === 'tauri' ? createTauriRuntime() : createWebRuntime();
}

const PlatformContext = createContext<PlatformRuntime | undefined>(undefined);

export function PlatformProvider({ children, runtime = createPlatformRuntime() }: { children: ReactNode; runtime?: PlatformRuntime }) {
  return <PlatformContext.Provider value={runtime}>{children}</PlatformContext.Provider>;
}

export function usePlatform() {
  const runtime = useContext(PlatformContext);
  if (!runtime) throw new Error('PlatformProvider is missing');
  return runtime;
}
