import { afterEach, describe, expect, it, vi } from 'vitest';
import { createPlatformRuntime, externalActionUrl, externalHttpUrl, isTauriRuntime } from './runtime';

afterEach(() => vi.unstubAllGlobals());

describe('platform runtime', () => {
  it('detects the Tauri runtime only from its injected internals', () => {
    expect(isTauriRuntime({})).toBe(false);
    expect(isTauriRuntime({ __TAURI_INTERNALS__: {} })).toBe(true);
  });

  it('only permits external HTTP URLs', () => {
    expect(externalHttpUrl('https://accounts.google.com/login')).toBe('https://accounts.google.com/login');
    expect(() => externalHttpUrl('file:///etc/passwd')).toThrow('只允许打开');
    expect(() => externalHttpUrl('javascript:alert(1)')).toThrow('只允许打开');
  });

  it('permits only sanitized mail-body action protocols', () => {
    expect(externalActionUrl('mailto:owner@example.com')).toBe('mailto:owner@example.com');
    expect(externalActionUrl('tel:+8613800000000')).toBe('tel:+8613800000000');
    expect(() => externalActionUrl('file:///etc/passwd')).toThrow('不支持打开');
    expect(() => externalActionUrl('javascript:alert(1)')).toThrow('不支持打开');
  });

  it('falls back to copying a shared link when system sharing is unavailable', async () => {
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal('window', {
      location: { href: 'https://mail.example.test/inbox', assign: vi.fn() },
      history: { state: null, replaceState: vi.fn() },
      setTimeout,
    });
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    await expect(createPlatformRuntime('web').share?.({ title: 'Mail', url: 'https://example.test/path' })).resolves.toBe('copied');
    expect(writeText).toHaveBeenCalledWith('https://example.test/path');
  });

  it('downloads a remote image through the authenticated safe-image endpoint', async () => {
    const click = vi.fn();
    vi.stubGlobal('window', {
      location: { href: 'https://mail.example.test/inbox', origin: 'https://mail.example.test' },
      history: { state: null, replaceState: vi.fn() },
      setTimeout,
    });
    vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn() } });
    vi.stubGlobal('document', { createElement: vi.fn(() => ({ click })) });
    const fetcher = vi.fn(async () => new Response(new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]), { headers: { 'Content-Type': 'image/png' } }));
    vi.stubGlobal('fetch', fetcher);
    await createPlatformRuntime('web').saveImage({ url: 'https://cdn.example.test/photo.png', filename: 'photo.png' });
    expect(fetcher).toHaveBeenCalledWith('https://mail.example.test/api/resources/image-download', expect.objectContaining({
      method: 'POST', credentials: 'include', body: JSON.stringify({ url: 'https://cdn.example.test/photo.png' }),
    }));
    expect(click).toHaveBeenCalledOnce();
  });
});
