import { describe, expect, it, vi } from 'vitest';
import { desktopDownload, desktopHttpRequest, desktopReadBinary, desktopTestService, type DesktopHttpInvoker } from './desktop-http';

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

  it('reads protected binary resources through the shared desktop session', async () => {
    const invokeMock = vi.fn(async () => [1, 2, 255]);
    const result = await desktopReadBinary('/api/contacts/logo?address=sender%40example.com', invokeMock as DesktopHttpInvoker);
    expect(Array.from(result)).toEqual([1, 2, 255]);
    expect(invokeMock).toHaveBeenCalledWith('desktop_read_binary', {
      baseUrl: expect.any(String), path: '/api/contacts/logo?address=sender%40example.com',
    });
  });

  it('keeps local binary reads and downloads inside the embedded Rust service', async () => {
    const invokeMock = vi.fn(async (command: string) => command === 'desktop_read_embedded_binary' ? [4, 5, 6] : undefined);
    const invoke = invokeMock as DesktopHttpInvoker;
    await expect(desktopReadBinary('/api/contacts/logo?address=local%40example.com', invoke, true)).resolves.toEqual(Uint8Array.from([4, 5, 6]));
    await desktopDownload('/api/messages/local/attachments/0', 'C:\\Temp\\local.bin', invoke, true);
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'desktop_read_embedded_binary', { path: '/api/contacts/logo?address=local%40example.com' });
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'desktop_download_embedded', { path: '/api/messages/local/attachments/0', target: 'C:\\Temp\\local.bin' });
  });
});
