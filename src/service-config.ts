export const SERVICE_URL_STORAGE_KEY = 'imail.service-url';
export const DEFAULT_SERVICE_URL = 'http://127.0.0.1:8787';

const buildTimeServiceUrl = String((import.meta as ImportMeta & { env?: Record<string, string> }).env?.VITE_API_BASE_URL ?? '').trim();

export function normalizeServiceUrl(value: string) {
  const trimmed = value.trim();
  if (!trimmed) return '';
  const url = new URL(trimmed);
  if (!['http:', 'https:'].includes(url.protocol)) throw new Error('服务地址只支持 HTTP 或 HTTPS');
  if (url.username || url.password || url.search || url.hash) throw new Error('服务地址不能包含凭据、查询参数或片段');
  return url.href.replace(/\/$/, '');
}

export function configuredServiceUrl(storage?: Pick<Storage, 'getItem'>) {
  const selected = storage ?? (typeof localStorage === 'undefined' ? undefined : localStorage);
  const stored = selected?.getItem(SERVICE_URL_STORAGE_KEY) ?? null;
  return normalizeServiceUrl(stored === null ? buildTimeServiceUrl || DEFAULT_SERVICE_URL : stored || DEFAULT_SERVICE_URL);
}

export function saveServiceUrl(value: string, storage?: Pick<Storage, 'setItem'>) {
  const normalized = normalizeServiceUrl(value);
  const selected = storage ?? localStorage;
  selected.setItem(SERVICE_URL_STORAGE_KEY, normalized);
  return normalized;
}

export function serviceUrl(path: string) {
  return `${configuredServiceUrl()}${path}`;
}

export function absoluteServiceUrl(path: string) {
  return `${configuredServiceUrl() || window.location.origin}${path}`;
}
