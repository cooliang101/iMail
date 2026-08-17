import { createContext, useContext, type ReactNode } from 'react';
import type { DownloadRequest, NotificationTarget, PlatformRuntime, SystemNotification } from './types';
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
  const clickListeners = new Set<(target: NotificationTarget) => void>();
  const dispatchClick = (target: NotificationTarget | undefined) => {
    if (!target?.messageId) return;
    for (const listener of clickListeners) listener(target);
  };
  const initialUrl = new URL(window.location.href);
  const initialMessageId = initialUrl.searchParams.get('notificationMessageId');
  const initialTarget = initialMessageId ? {
    messageId: initialMessageId,
    accountEmail: initialUrl.searchParams.get('notificationAccountEmail') || undefined,
  } : undefined;
  if (initialMessageId) {
    initialUrl.searchParams.delete('notificationMessageId');
    initialUrl.searchParams.delete('notificationAccountEmail');
    window.history.replaceState(window.history.state, '', initialUrl);
  }
  const onServiceWorkerMessage = (event: MessageEvent) => {
    const data = event.data as { type?: string; target?: NotificationTarget } | undefined;
    if (data?.type === 'imail-notification-click') dispatchClick(data.target);
  };
  navigator.serviceWorker?.addEventListener('message', onServiceWorkerMessage);

  async function prepareNotifications() {
    if (!('Notification' in window)) return false;
    if (Notification.permission === 'granted') return true;
    if (Notification.permission === 'denied') return false;
    return await Notification.requestPermission() === 'granted';
  }

  return {
    kind: 'web',
    async openExternal(value) {
      const opened = window.open(externalHttpUrl(value), '_blank', 'noopener,noreferrer');
      if (!opened) throw new Error('浏览器阻止了新窗口，请允许弹出窗口后重试');
    },
    saveDownload: webDownload,
    prepareNotifications,
    async notify({ title, body, tag, target }: SystemNotification) {
      if (!await prepareNotifications()) throw new Error('系统通知权限未开启');
      const options: NotificationOptions = { body, tag, data: { target }, icon: new URL('pwa-192.png', document.baseURI).href };
      const registration = 'serviceWorker' in navigator
        ? await navigator.serviceWorker.getRegistration().catch(() => undefined)
        : undefined;
      if (registration) {
        await registration.showNotification(title, options);
        return;
      }
      const notification = new Notification(title, options);
      notification.onclick = () => { window.focus(); notification.close(); dispatchClick(target); };
    },
    subscribeNotificationClicks(listener) {
      clickListeners.add(listener);
      if (initialTarget) queueMicrotask(() => {
        if (clickListeners.has(listener)) listener(initialTarget);
      });
      return () => { clickListeners.delete(listener); };
    },
  };
}

function createTauriRuntime(): PlatformRuntime {
  async function prepareNotifications() {
    const { isPermissionGranted, requestPermission } = await import('@tauri-apps/plugin-notification');
    return await isPermissionGranted() || await requestPermission() === 'granted';
  }

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
    prepareNotifications,
    async notify(input) {
      if (!await prepareNotifications()) throw new Error('系统通知权限未开启');
      if (input.target) {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('desktop_notify_message', { input });
        return;
      }
      const { sendNotification } = await import('@tauri-apps/plugin-notification');
      sendNotification({ title: input.title, body: input.body });
    },
    subscribeNotificationClicks(listener) {
      let closed = false;
      let unlisten: (() => void) | undefined;
      void import('@tauri-apps/api/event').then(async ({ listen }) => {
        const dispose = await listen<NotificationTarget>('imail-notification-click', ({ payload }) => listener(payload));
        if (closed) dispose();
        else unlisten = dispose;
      }).catch((error) => console.error('[notification-listener]', error));
      return () => { closed = true; unlisten?.(); };
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
