import { describe, expect, it } from 'vitest';
import { configuredServiceUrl, DEFAULT_SERVICE_URL, normalizeServiceUrl, saveServiceUrl, SERVICE_URL_STORAGE_KEY } from './service-config';

function memoryStorage(initial?: string) {
  const values = new Map<string, string>();
  if (initial !== undefined) values.set(SERVICE_URL_STORAGE_KEY, initial);
  return {
    getItem(key: string) { return values.get(key) ?? null; },
    setItem(key: string, value: string) { values.set(key, value); },
  };
}

describe('service configuration', () => {
  it('uses the local independent service by default', () => {
    expect(configuredServiceUrl(memoryStorage())).toBe(DEFAULT_SERVICE_URL);
  });

  it('normalizes HTTP service addresses and rejects unsafe URL forms', () => {
    expect(normalizeServiceUrl(' https://mail.example.com/ ')).toBe('https://mail.example.com');
    expect(() => normalizeServiceUrl('file:///tmp/imail')).toThrow('HTTP');
    expect(() => normalizeServiceUrl('https://user:secret@mail.example.com')).toThrow('凭据');
  });

  it('persists the service address independently from feature API paths', () => {
    const storage = memoryStorage();
    expect(saveServiceUrl('https://mail.example.com/', storage)).toBe('https://mail.example.com');
    expect(configuredServiceUrl(storage)).toBe('https://mail.example.com');
  });
});
