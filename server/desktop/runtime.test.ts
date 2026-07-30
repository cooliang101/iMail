import { describe, expect, it } from 'vitest';
import { DESKTOP_CONTENT_SECURITY_POLICY, desktopRuntimeConfig, desktopRuntimeMiddleware, desktopStartupHealth } from './runtime.js';

function responseRecorder() {
  const result = { statusCode: 200, body: undefined as unknown };
  return {
    result,
    response: {
      status(code: number) { result.statusCode = code; return this; },
      json(body: unknown) { result.body = body; return this; },
    },
  };
}

describe('desktop runtime', () => {
  it('is disabled unless explicitly enabled', () => {
    expect(desktopRuntimeConfig({})).toEqual({ enabled: false, startupToken: undefined, webDirectory: undefined });
    expect(desktopRuntimeConfig({ IMAIL_DESKTOP_MODE: 'true', IMAIL_WEB_DIR: '.desktop-web' })).toMatchObject({ enabled: true, webDirectory: expect.stringContaining('.desktop-web') });
  });

  it('requires the exact startup token for desktop health', () => {
    const handler = desktopStartupHealth({ enabled: true, startupToken: 'desktop-secret' });
    const rejected = responseRecorder();
    handler({ header: () => 'wrong' } as never, rejected.response as never, () => undefined);
    expect(rejected.result).toMatchObject({ statusCode: 401, body: { error: '桌面启动握手失败' } });

    const accepted = responseRecorder();
    handler({ header: () => 'desktop-secret' } as never, accepted.response as never, () => undefined);
    expect(accepted.result).toMatchObject({ statusCode: 200, body: { ok: true, service: 'imail-desktop' } });
  });

  it('restricts desktop traffic to loopback hosts and applies browser security headers', () => {
    const middleware = desktopRuntimeMiddleware({ enabled: true });
    const headers = new Map<string, string>();
    let continued = false;
    middleware(
      { hostname: '127.0.0.1' } as never,
      { setHeader(name: string, value: string) { headers.set(name, value); } } as never,
      () => { continued = true; },
    );
    expect(continued).toBe(true);
    expect(headers.get('Content-Security-Policy')).toBe(DESKTOP_CONTENT_SECURITY_POLICY);
    expect(headers.get('X-Content-Type-Options')).toBe('nosniff');

    const rejected = responseRecorder();
    middleware({ hostname: 'mail.example.com' } as never, rejected.response as never, () => undefined);
    expect(rejected.result).toMatchObject({ statusCode: 421, body: { error: '桌面服务只接受本机回环地址' } });
  });
});
