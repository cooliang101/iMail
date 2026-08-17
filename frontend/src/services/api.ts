import { serviceUrl } from './config';
import { isTauriRuntime } from '../platform/tauri-runtime';
import { createMailService } from './mail';
import { desktopLog, describeDesktopLogValue } from './desktop/logging';

export type ApiTransport = {
  request<T>(path: string, options?: RequestInit): Promise<T>;
};

async function responseJson<T>(response: Response, path: string): Promise<T> {
  try {
    return await response.json() as T;
  } catch {
    throw new Error(`服务返回了无法解析的数据：${path.split('?')[0]}`);
  }
}

export function createApiTransport({ baseUrl = '', fetcher = fetch }: { baseUrl?: string | (() => string); fetcher?: typeof fetch } = {}): ApiTransport {
  return { async request<T>(path: string, options?: RequestInit): Promise<T> {
  const resolvedBaseUrl = typeof baseUrl === 'function' ? baseUrl() : baseUrl;
  const response = await fetcher(`${resolvedBaseUrl}${path}`, {
    ...options,
    credentials: 'include',
    headers: { 'Content-Type': 'application/json', ...options?.headers },
  });
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: response.statusText })) as { error?: string };
    const error = new Error(typeof body.error === 'string' && body.error.trim() ? body.error : response.statusText || '请求失败') as Error & { status?: number };
    error.status = response.status;
    if (response.status === 401 && !path.startsWith('/api/auth/')) window.dispatchEvent(new Event('imail:unauthorized'));
    throw error;
  }
  if (response.status === 204) return undefined as T;
  return responseJson<T>(response, path);
  } };
}

const defaultTransport = createApiTransport({ baseUrl: () => serviceUrl('') });

async function desktopApi<T>(path: string, options?: RequestInit) {
  const response = await createMailService().request(path, options);
  if (response.status < 200 || response.status >= 300) {
    let message = `请求失败（${response.status}）`;
    try {
      const parsed = JSON.parse(response.body) as { error?: unknown };
      if (typeof parsed.error === 'string' && parsed.error.trim()) message = parsed.error;
    } catch { /* Keep the status message for a non-JSON error. */ }
    const error = new Error(message) as Error & { status?: number };
    error.status = response.status;
    if (response.status === 401 && !path.startsWith('/api/auth/')) window.dispatchEvent(new Event('imail:unauthorized'));
    throw error;
  }
  if (response.status === 204 || !response.body) return undefined as T;
  try {
    return JSON.parse(response.body) as T;
  } catch {
    throw new Error(`服务返回了无法解析的数据：${path.split('?')[0]}`);
  }
}

export async function api<T>(path: string, options?: RequestInit): Promise<T> {
  try {
    return await (isTauriRuntime() ? desktopApi<T>(path, options) : defaultTransport.request<T>(path, options));
  } catch (reason) {
    const error = reason instanceof Error
      ? reason
      : new Error(typeof reason === 'string' && reason.trim() ? reason : `请求失败：${path.split('?')[0]}`);
    const status = 'status' in error && typeof error.status === 'number' ? ` status=${error.status}` : '';
    void desktopLog('error', 'api.request_failed', `method=${options?.method ?? 'GET'} path=${path.split('?')[0]}${status}\n${describeDesktopLogValue(error)}`);
    throw error;
  }
}
