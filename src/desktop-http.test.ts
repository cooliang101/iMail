import { describe, expect, it, vi } from 'vitest';
import { desktopDownload, desktopHttpRequest, desktopTestService, type DesktopHttpInvoker } from './desktop-http';

describe('desktop HTTP bridge', () => {
  it('sends API requests to the Rust command instead of browser fetch', async () => {
    const invokeMock = vi.fn(async (_command: string, _args?: Record<string, unknown>) => ({ status: 200, body: '{"ok":true}' }));
    await expect(desktopHttpRequest('/api/health', { method: 'POST', body: '{"check":true}' }, 'http://127.0.0.1:8787', invokeMock as DesktopHttpInvoker)).resolves.toEqual({ status: 200, body: '{"ok":true}' });
    expect(invokeMock).toHaveBeenCalledWith('desktop_http_request', { request: {
      baseUrl: 'http://127.0.0.1:8787', path: '/api/health', method: 'POST', body: '{"check":true}', timeoutMs: 30_000,
    } });
  });

  it('uses the Rust bridge for connection tests and downloads', async () => {
    const invokeMock = vi.fn(async (_command: string, _args?: Record<string, unknown>) => ({ status: 200, body: '{}' }));
    const invoke = invokeMock as DesktopHttpInvoker;
    await desktopTestService('https://mail.example.com', invoke);
    await desktopDownload('/api/attachment/1', 'C:\\Temp\\mail.pdf', invoke);
    expect(invokeMock.mock.calls[0][0]).toBe('desktop_http_request');
    expect(invokeMock.mock.calls[1][0]).toBe('desktop_download');
  });
});
