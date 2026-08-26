import { serviceUrl } from './config';
import { isTauriRuntime } from '../platform/tauri-runtime';
import { createMailService } from './mail';
import { desktopLog, describeDesktopLogValue } from './desktop/logging';

export type ApiTransport = {
  request<T>(path: string, options?: RequestInit): Promise<T>;
};

function isApplicationSessionFailure(status: number, path: string) {
  return status === 401 && !path.startsWith('/api/auth/') && !path.includes('/apple-hme');
}

async function responseJson<T>(response: Response, _path: string): Promise<T> {
  try {
    return await response.json() as T;
  } catch {
    throw new Error('iMail 收到的数据格式不正确，请重试');
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
    if (isApplicationSessionFailure(response.status, path)) window.dispatchEvent(new Event('imail:unauthorized'));
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
    if (isApplicationSessionFailure(response.status, path)) window.dispatchEvent(new Event('imail:unauthorized'));
    throw error;
  }
  if (response.status === 204 || !response.body) return undefined as T;
  try {
    return JSON.parse(response.body) as T;
  } catch {
    throw new Error('iMail 收到的数据格式不正确，请重试');
  }
}

export function userFacingErrorMessage(reason: unknown, fallback = 'iMail 暂时无法完成请求，请稍后重试') {
  const raw = reason instanceof Error ? reason.message : typeof reason === 'string' ? reason : fallback;
  if (!raw.trim()) return fallback;
  if (/尚未映射|领域调用未实现/i.test(raw)) return 'iMail 暂时无法完成这项操作，请更新应用后重试';
  return raw
    .replace(/初始化嵌入式 Rust 服务/g, '初始化 iMail 本地服务')
    .replace(/Tauri 直连/gi, 'iMail')
    .replace(/嵌入式登录状态/g, ' iMail 登录状态')
    .replace(/嵌入式用户状态/g, ' iMail 用户状态')
    .replace(/嵌入式 Rust 服务/g, ' iMail 本地服务')
    .replace(/嵌入式服务/g, ' iMail 本地服务')
    .replace(/Rust 数据/g, 'iMail 数据')
    .replace(/\b(?:GET|POST|PUT|PATCH|DELETE)\s+\/api\/\S+/g, '当前操作')
    .trim();
}

export async function api<T>(path: string, options?: RequestInit): Promise<T> {
  try {
    return await (isTauriRuntime() ? desktopApi<T>(path, options) : defaultTransport.request<T>(path, options));
  } catch (reason) {
    const source = reason instanceof Error ? reason : new Error(typeof reason === 'string' ? reason : '');
    const error = new Error(userFacingErrorMessage(source)) as Error & { status?: number };
    if ('status' in source && typeof source.status === 'number') error.status = source.status;
    const status = 'status' in error && typeof error.status === 'number' ? ` status=${error.status}` : '';
    void desktopLog('error', 'api.request_failed', `method=${options?.method ?? 'GET'} path=${path.split('?')[0]}${status}\n${describeDesktopLogValue(error)}`);
    throw error;
  }
}
