import { createContext, useContext, type ReactNode } from 'preact/compat';
import type { DownloadRequest, NotificationTarget, PlatformRuntime, ShareRequest, SystemNotification, TextSaveRequest } from './types';
import { absoluteServiceUrl, desktopDownload } from '../services';
import { isTauriRuntime } from './tauri-runtime';
export { isTauriRuntime } from './tauri-runtime';
export function externalHttpUrl(value: string) {
  const url = new URL(value);
  if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error('只允许打开 HTTP 或 HTTPS 地址');
  return url.href;
}

export function externalActionUrl(value: string) {
  const url = new URL(value);
  if (!['http:', 'https:', 'mailto:', 'tel:'].includes(url.protocol.toLocaleLowerCase())) throw new Error('不支持打开此类链接');
  return url.href;
}

async function webDownload({ url, filename }: DownloadRequest) {
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  anchor.rel = 'noreferrer';
  anchor.click();
}

const MAX_SAVED_IMAGE_BYTES = 25 * 1024 * 1024;
const rasterImageType = /^image\/(?:avif|bmp|gif|jpeg|png|webp)(?:;|$)/i;

function safeImageUrl(value: string) {
  if (/^data:image\/(?:avif|bmp|gif|jpeg|png|webp);base64,/i.test(value)) return value;
  const url = new URL(value, window.location.href);
  if (url.protocol === 'blob:' && url.origin === window.location.origin) return url.href;
  return externalHttpUrl(url.href);
}

async function fetchSavedImage(url: string) {
  const safeUrl = safeImageUrl(url);
  const remote = /^https?:/i.test(safeUrl);
  const response = await fetch(remote ? absoluteServiceUrl('/api/resources/image-download') : safeUrl, remote ? {
    method: 'POST', credentials: 'include', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ url: safeUrl }),
  } : { credentials: 'omit', referrerPolicy: 'no-referrer' });
  if (!response.ok) throw new Error(`图片下载失败（${response.status}）`);
  const contentType = response.headers.get('content-type') ?? '';
  if (!rasterImageType.test(contentType)) throw new Error('远程资源不是受支持的图片格式');
  const declaredSize = Number(response.headers.get('content-length'));
  if (Number.isFinite(declaredSize) && declaredSize > MAX_SAVED_IMAGE_BYTES) throw new Error('图片超过 25 MiB，无法保存');
  const blob = await response.blob();
  if (blob.size > MAX_SAVED_IMAGE_BYTES) throw new Error('图片超过 25 MiB，无法保存');
  return blob;
}

async function webSaveImage({ url, filename }: DownloadRequest) {
  const blob = await fetchSavedImage(url);
  const blobUrl = URL.createObjectURL(blob);
  try { await webDownload({ url: blobUrl, filename }); }
  finally { window.setTimeout(() => URL.revokeObjectURL(blobUrl), 0); }
}

async function webSaveText({ text, filename }: TextSaveRequest) {
  const blobUrl = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
  try { await webDownload({ url: blobUrl, filename }); }
  finally { window.setTimeout(() => URL.revokeObjectURL(blobUrl), 0); }
}

async function shareWithFallback(input: ShareRequest) {
  if (typeof navigator.share === 'function') {
    try { await navigator.share(input); return 'shared' as const; }
    catch (reason) { if (reason instanceof DOMException && reason.name === 'AbortError') throw reason; }
  }
  const value = input.url || input.text;
  if (!value) throw new Error('没有可分享的内容');
  await navigator.clipboard.writeText(value);
  return 'copied' as const;
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
      const url = externalActionUrl(value);
      if (!/^https?:/i.test(url)) { window.location.assign(url); return; }
      const opened = window.open(externalHttpUrl(url), '_blank', 'noopener,noreferrer');
      if (!opened) throw new Error('浏览器阻止了新窗口，请允许弹出窗口后重试');
    },
    share: shareWithFallback,
    saveDownload: webDownload,
    saveImage: webSaveImage,
    saveText: webSaveText,
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
      await openUrl(externalActionUrl(value));
    },
    share: shareWithFallback,
    async saveDownload({ url, filename }) {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const target = await save({ defaultPath: filename });
      if (!target) return;
      await desktopDownload(url, target);
    },
    async saveImage({ url, filename }) {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const target = await save({ defaultPath: filename });
      if (!target) return;
      const { invoke } = await import('@tauri-apps/api/core');
      if (/^https?:/i.test(url)) {
        await invoke('desktop_download_external_image', { url: externalHttpUrl(url), target });
        return;
      }
      const bytes = new Uint8Array(await (await fetchSavedImage(url)).arrayBuffer());
      await invoke('desktop_save_binary', { bytes: Array.from(bytes), target });
    },
    async saveText({ text, filename }) {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const target = await save({ defaultPath: filename });
      if (!target) return;
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('desktop_save_text', { text, target });
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
