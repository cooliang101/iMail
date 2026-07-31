import { serviceUrl } from './service-config';
import { desktopHttpRequest } from './desktop-http';
import { isTauriRuntime } from './platform/tauri-runtime';

export type ApiTransport = {
  request<T>(path: string, options?: RequestInit): Promise<T>;
};

export function createApiTransport({ baseUrl = '', fetcher = fetch }: { baseUrl?: string | (() => string); fetcher?: typeof fetch } = {}): ApiTransport {
  return { async request<T>(path: string, options?: RequestInit): Promise<T> {
  const resolvedBaseUrl = typeof baseUrl === 'function' ? baseUrl() : baseUrl;
  const response = await fetcher(`${resolvedBaseUrl}${path}`, {
    ...options,
    credentials: 'include',
    headers: { 'Content-Type': 'application/json', ...options?.headers },
  });
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: response.statusText }));
    const error = new Error(body.error ?? '请求失败') as Error & { status?: number };
    error.status = response.status;
    if (response.status === 401 && !path.startsWith('/api/auth/')) window.dispatchEvent(new Event('imail:unauthorized'));
    throw error;
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
  } };
}

const defaultTransport = createApiTransport({ baseUrl: () => serviceUrl('') });

async function desktopApi<T>(path: string, options?: RequestInit) {
  const response = await desktopHttpRequest(path, options);
  if (response.status < 200 || response.status >= 300) {
    let message = `请求失败（${response.status}）`;
    try { message = JSON.parse(response.body).error ?? message; } catch { /* Keep the status message for a non-JSON error. */ }
    const error = new Error(message) as Error & { status?: number };
    error.status = response.status;
    if (response.status === 401 && !path.startsWith('/api/auth/')) window.dispatchEvent(new Event('imail:unauthorized'));
    throw error;
  }
  if (response.status === 204 || !response.body) return undefined as T;
  return JSON.parse(response.body) as T;
}

export function api<T>(path: string, options?: RequestInit): Promise<T> {
  return isTauriRuntime() ? desktopApi<T>(path, options) : defaultTransport.request<T>(path, options);
}
