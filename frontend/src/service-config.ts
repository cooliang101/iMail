export type ServiceMode = 'local' | 'remote';
export type ServiceSelection = { mode: 'local' } | { mode: 'remote'; remoteUrl: string };

export const SERVICE_MODE_STORAGE_KEY = 'imail.service-mode';
export const SERVICE_URL_STORAGE_KEY = 'imail.service-url';
export const LEGACY_LOCAL_SERVICE_URL = 'http://127.0.0.1:8787';
export const LOCAL_SERVICE_URL = 'tauri://embedded';
export const DEFAULT_SERVICE_URL = LOCAL_SERVICE_URL;

export function embeddedTauriServiceEnabled() {
  return typeof window !== 'undefined'
    && (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== undefined;
}

const buildTimeServiceUrl = String((import.meta as ImportMeta & { env?: Record<string, string> }).env?.VITE_API_BASE_URL ?? '').trim();

type ReadStorage = Pick<Storage, 'getItem'>;
type WriteStorage = Pick<Storage, 'getItem' | 'setItem'>;

function storageOrDefault(storage?: ReadStorage) {
  return storage ?? (typeof localStorage === 'undefined' ? undefined : localStorage);
}

function sameOriginWebService() {
  if (typeof window === 'undefined' || (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== undefined) return '';
  return window.location.origin;
}

export function normalizeServiceUrl(value: string) {
  const trimmed = value.trim();
  if (!trimmed) return '';
  const url = new URL(trimmed);
  if (!['http:', 'https:'].includes(url.protocol)) throw new Error('服务地址只支持 HTTP 或 HTTPS');
  if (url.username || url.password || url.search || url.hash) throw new Error('服务地址不能包含凭据、查询参数或片段');
  return url.href.replace(/\/$/, '');
}

function isLoopbackServiceHost(hostname: string) {
  const normalized = hostname.toLowerCase().replace(/^\[|\]$/g, '');
  if (normalized === 'localhost' || normalized === '::1') return true;
  return /^127(?:\.\d{1,3}){3}$/.test(normalized);
}

export function secureRemoteServiceUrl(value: string) {
  const normalized = normalizeServiceUrl(value);
  if (!normalized) throw new Error('请输入 iMail 服务地址');
  const url = new URL(normalized);
  if (url.protocol === 'https:' || (url.protocol === 'http:' && isLoopbackServiceHost(url.hostname))) return normalized;
  throw new Error('远程服务必须使用 HTTPS；HTTP 仅允许本机回环地址');
}

export function configuredLocalServiceUrl(_storage?: ReadStorage) {
  return LOCAL_SERVICE_URL;
}

export function configuredServiceMode(storage?: ReadStorage): ServiceMode {
  const selected = storageOrDefault(storage);
  const explicit = selected?.getItem(SERVICE_MODE_STORAGE_KEY);
  if (explicit === 'local' || explicit === 'remote') return explicit;

  // Preserve remote endpoints saved by older clients while treating the former loopback daemon as local.
  const legacyUrl = normalizeServiceUrl(selected?.getItem(SERVICE_URL_STORAGE_KEY) ?? '');
  if (legacyUrl && legacyUrl !== LEGACY_LOCAL_SERVICE_URL) return 'remote';
  return sameOriginWebService() ? 'remote' : 'local';
}

export function configuredRemoteServiceUrl(storage?: ReadStorage) {
  const selected = storageOrDefault(storage);
  const stored = normalizeServiceUrl(selected?.getItem(SERVICE_URL_STORAGE_KEY) ?? '');
  if (stored && stored !== LEGACY_LOCAL_SERVICE_URL) return stored;
  const built = normalizeServiceUrl(buildTimeServiceUrl);
  if (built && built !== LEGACY_LOCAL_SERVICE_URL) return built;
  return normalizeServiceUrl(sameOriginWebService());
}

export function configuredServiceUrl(storage?: ReadStorage) {
  if (configuredServiceMode(storage) === 'local') return configuredLocalServiceUrl(storage);
  return configuredRemoteServiceUrl(storage) || LOCAL_SERVICE_URL;
}

export function saveServiceSelection(selection: ServiceSelection, storage?: WriteStorage) {
  const selected = storage ?? localStorage;
  if (selection.mode === 'remote') {
    const remoteUrl = secureRemoteServiceUrl(selection.remoteUrl);
    if (remoteUrl === LEGACY_LOCAL_SERVICE_URL) throw new Error('远程服务地址不能使用旧版本机守护地址');
    selected.setItem(SERVICE_URL_STORAGE_KEY, remoteUrl);
  }
  selected.setItem(SERVICE_MODE_STORAGE_KEY, selection.mode);
  return selection.mode === 'local' ? configuredLocalServiceUrl(selected) : configuredRemoteServiceUrl(selected);
}

export function saveServiceUrl(value: string, storage?: Pick<Storage, 'setItem'>) {
  const normalized = secureRemoteServiceUrl(value);
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
