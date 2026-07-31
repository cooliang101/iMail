import { configuredServiceUrl } from './service-config';

export type DesktopHttpResponse = { status: number; body: string };
export type DesktopHttpInvoker = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

async function tauriInvoke<T>(command: string, args?: Record<string, unknown>) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

export async function desktopHttpRequest(path: string, options: RequestInit = {}, baseUrl = configuredServiceUrl(), invoker: DesktopHttpInvoker = tauriInvoke) {
  if (options.body !== undefined && typeof options.body !== 'string') throw new Error('桌面 API 只接受 JSON 请求体');
  return invoker<DesktopHttpResponse>('desktop_http_request', {
    request: {
      baseUrl,
      path,
      method: options.method ?? 'GET',
      body: options.body,
      timeoutMs: 30_000,
    },
  });
}

export async function desktopTestService(baseUrl: string, invoker: DesktopHttpInvoker = tauriInvoke) {
  return invoker<DesktopHttpResponse>('desktop_http_request', {
    request: { baseUrl, path: '/api/auth/status', method: 'GET', timeoutMs: 8_000 },
  });
}

export async function desktopDownload(path: string, target: string, invoker: DesktopHttpInvoker = tauriInvoke) {
  await invoker('desktop_download', { baseUrl: configuredServiceUrl(), path, target });
}
