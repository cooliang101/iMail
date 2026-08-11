import { describe, expect, it, vi } from 'vitest';
import type { DesktopHttpInvoker } from '../../desktop-http';
import { parseServiceInfo, serviceErrorMessage, testServiceConnection } from './service-connection';

const info = {
  service: 'imail' as const,
  instanceId: '11111111-1111-4111-8111-111111111111',
  version: '0.1.0',
  protocolVersion: 1,
  capabilities: { gateway: true, mcp: true, syncWorker: true, webClient: false },
};

describe('service connection contract', () => {
  it('preserves string errors returned by Tauri commands', () => {
    expect(serviceErrorMessage('安装包缺少管理程序', 'fallback')).toBe('安装包缺少管理程序');
    expect(serviceErrorMessage(new Error('连接失败'), 'fallback')).toBe('连接失败');
    expect(serviceErrorMessage(undefined, 'fallback')).toBe('fallback');
  });

  it('accepts a compatible iMail service identity', () => {
    expect(parseServiceInfo(info)).toEqual(info);
  });

  it('rejects unknown services and incompatible protocols', () => {
    expect(() => parseServiceInfo({ ...info, service: 'other' })).toThrow('不是 iMail');
    expect(() => parseServiceInfo({ ...info, protocolVersion: 2 })).toThrow('不兼容');
  });

  it('tests desktop services through the Rust bridge', async () => {
    const invoker = vi.fn(async () => ({ status: 200, body: JSON.stringify(info) }));
    await expect(testServiceConnection('http://127.0.0.1:8787', { desktop: true, invoker: invoker as DesktopHttpInvoker })).resolves.toEqual(info);
    expect(invoker).toHaveBeenCalledWith('desktop_http_request', { request: {
      baseUrl: 'http://127.0.0.1:8787', path: '/api/system/info', method: 'GET', timeoutMs: 8_000,
    } });
  });

  it('tests embedded local identity through the typed command without a base URL', async () => {
    const invoker = vi.fn(async () => ({ status: 200, body: JSON.stringify(info) }));
    await expect(testServiceConnection('http://127.0.0.1:8787', {
      desktop: true, embeddedLocal: true, invoker: invoker as DesktopHttpInvoker,
    })).resolves.toEqual(info);
    expect(invoker).toHaveBeenCalledWith('desktop_mail_service_call', { call: { operation: 'systemInfo' } });
  });

  it('does not accept an arbitrary healthy HTTP endpoint', async () => {
    const fetcher = vi.fn(async () => new Response(JSON.stringify({ ok: true }), { status: 200, headers: { 'Content-Type': 'application/json' } }));
    await expect(testServiceConnection('https://example.com', { desktop: false, fetcher })).rejects.toThrow('不是 iMail');
  });
});
