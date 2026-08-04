import type { ServiceInfo } from '../../types';
import { desktopTestService, type DesktopHttpInvoker } from '../../desktop-http';
import { isTauriRuntime } from '../../platform/tauri-runtime';

export const SUPPORTED_SERVICE_PROTOCOL_VERSION = 1;

export function serviceErrorMessage(reason: unknown, fallback: string) {
  if (reason instanceof Error && reason.message) return reason.message;
  if (typeof reason === 'string' && reason.trim()) return reason.trim();
  return fallback;
}

export function parseServiceInfo(value: unknown): ServiceInfo {
  if (!value || typeof value !== 'object') throw new Error('目标没有返回有效的 iMail 服务信息');
  const candidate = value as Partial<ServiceInfo>;
  if (candidate.service !== 'imail') throw new Error('目标不是 iMail 服务');
  if (typeof candidate.instanceId !== 'string' || !candidate.instanceId) throw new Error('iMail 服务缺少实例身份');
  if (typeof candidate.version !== 'string' || !candidate.version) throw new Error('iMail 服务缺少版本信息');
  if (candidate.protocolVersion !== SUPPORTED_SERVICE_PROTOCOL_VERSION) {
    throw new Error(`服务协议版本不兼容（需要 ${SUPPORTED_SERVICE_PROTOCOL_VERSION}，实际 ${String(candidate.protocolVersion ?? '未知')}）`);
  }
  if (!candidate.capabilities || typeof candidate.capabilities !== 'object') throw new Error('iMail 服务缺少能力信息');
  return candidate as ServiceInfo;
}

export async function testServiceConnection(baseUrl: string, options: {
  desktop?: boolean;
  invoker?: DesktopHttpInvoker;
  fetcher?: typeof fetch;
} = {}) {
  if (options.desktop ?? isTauriRuntime()) {
    const response = await desktopTestService(baseUrl, options.invoker);
    if (response.status < 200 || response.status >= 300) throw new Error(`服务返回 ${response.status}`);
    try { return parseServiceInfo(JSON.parse(response.body)); }
    catch (error) {
      if (error instanceof SyntaxError) throw new Error('目标没有返回 JSON 格式的 iMail 服务信息');
      throw error;
    }
  }

  const response = await (options.fetcher ?? fetch)(`${baseUrl}/api/system/info`, {
    credentials: 'include', signal: AbortSignal.timeout(8_000),
  });
  if (!response.ok) throw new Error(`服务返回 ${response.status}`);
  return parseServiceInfo(await response.json());
}
