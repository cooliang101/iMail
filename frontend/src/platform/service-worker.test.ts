import { describe, expect, it } from 'vitest';
import { shouldRegisterServiceWorker } from './service-worker';

describe('web service worker registration', () => {
  it('registers only for production web pages served over HTTP(S)', () => {
    expect(shouldRegisterServiceWorker({ production: true, tauri: false, protocol: 'https:', supported: true })).toBe(true);
    expect(shouldRegisterServiceWorker({ production: false, tauri: false, protocol: 'http:', supported: true })).toBe(false);
    expect(shouldRegisterServiceWorker({ production: true, tauri: true, protocol: 'https:', supported: true })).toBe(false);
    expect(shouldRegisterServiceWorker({ production: true, tauri: false, protocol: 'tauri:', supported: true })).toBe(false);
    expect(shouldRegisterServiceWorker({ production: true, tauri: false, protocol: 'https:', supported: false })).toBe(false);
  });
});
