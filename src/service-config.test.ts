import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  configuredRemoteServiceUrl,
  configuredLocalServiceSuspended,
  configuredServiceMode,
  configuredServiceUrl,
  LOCAL_SERVICE_URL,
  normalizeServiceUrl,
  secureRemoteServiceUrl,
  saveServiceSelection,
  saveLocalServiceSuspended,
  saveServiceUrl,
  SERVICE_MODE_STORAGE_KEY,
  LOCAL_SERVICE_SUSPENDED_STORAGE_KEY,
  SERVICE_URL_STORAGE_KEY,
} from './service-config';

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem(key: string) { return values.get(key) ?? null; },
    setItem(key: string, value: string) { values.set(key, value); },
  };
}

describe('service configuration', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('uses local mode and the loopback service by default', () => {
    const storage = memoryStorage();
    expect(configuredServiceMode(storage)).toBe('local');
    expect(configuredServiceUrl(storage)).toBe(LOCAL_SERVICE_URL);
  });

  it('migrates an existing non-loopback endpoint to remote mode', () => {
    const storage = memoryStorage({ [SERVICE_URL_STORAGE_KEY]: 'https://mail.example.com/' });
    expect(configuredServiceMode(storage)).toBe('remote');
    expect(configuredServiceUrl(storage)).toBe('https://mail.example.com');
  });

  it('uses the hosting origin by default in the browser web client', () => {
    vi.stubGlobal('window', { location: { origin: 'https://mail.example.com' } });
    const storage = memoryStorage();
    expect(configuredServiceMode(storage)).toBe('remote');
    expect(configuredServiceUrl(storage)).toBe('https://mail.example.com');
  });

  it('keeps an explicit mode independent from the saved remote endpoint', () => {
    const storage = memoryStorage({
      [SERVICE_MODE_STORAGE_KEY]: 'local',
      [SERVICE_URL_STORAGE_KEY]: 'https://mail.example.com',
    });
    expect(configuredServiceMode(storage)).toBe('local');
    expect(configuredRemoteServiceUrl(storage)).toBe('https://mail.example.com');
    expect(saveServiceSelection({ mode: 'remote', remoteUrl: 'https://remote.example.com/' }, storage)).toBe('https://remote.example.com');
    expect(configuredServiceMode(storage)).toBe('remote');
    expect(saveServiceSelection({ mode: 'local' }, storage)).toBe(LOCAL_SERVICE_URL);
    expect(configuredRemoteServiceUrl(storage)).toBe('https://remote.example.com');
  });

  it('normalizes HTTP service addresses and rejects unsafe URL forms', () => {
    expect(normalizeServiceUrl(' https://mail.example.com/ ')).toBe('https://mail.example.com');
    expect(() => normalizeServiceUrl('file:///tmp/imail')).toThrow('HTTP');
    expect(() => normalizeServiceUrl('https://user:secret@mail.example.com')).toThrow('凭据');
    expect(() => saveServiceSelection({ mode: 'remote', remoteUrl: LOCAL_SERVICE_URL }, memoryStorage())).toThrow('远程');
    expect(secureRemoteServiceUrl('http://localhost:18787/')).toBe('http://localhost:18787');
    expect(secureRemoteServiceUrl('http://127.20.30.40:18787')).toBe('http://127.20.30.40:18787');
    expect(secureRemoteServiceUrl('http://[::1]:18787')).toBe('http://[::1]:18787');
    expect(secureRemoteServiceUrl('https://192.168.1.20:8787')).toBe('https://192.168.1.20:8787');
    expect(() => secureRemoteServiceUrl('http://192.168.1.20:8787')).toThrow('HTTPS');
    expect(() => secureRemoteServiceUrl('http://mail.example.com')).toThrow('HTTPS');
  });

  it('keeps the legacy address writer independent from feature API paths', () => {
    const storage = memoryStorage();
    expect(saveServiceUrl('https://mail.example.com/', storage)).toBe('https://mail.example.com');
    expect(configuredRemoteServiceUrl(storage)).toBe('https://mail.example.com');
  });

  it('keeps an explicit local daemon pause until local mode is selected again', () => {
    const storage = memoryStorage();
    expect(configuredLocalServiceSuspended(storage)).toBe(false);
    expect(saveLocalServiceSuspended(true, storage)).toBe(true);
    expect(configuredLocalServiceSuspended(storage)).toBe(true);
    saveServiceSelection({ mode: 'local' }, storage);
    expect(configuredLocalServiceSuspended(storage)).toBe(false);
    expect(storage.getItem(LOCAL_SERVICE_SUSPENDED_STORAGE_KEY)).toBe('false');
  });
});
