import { describe, expect, it } from 'vitest';
import { externalHttpUrl, isTauriRuntime } from './runtime';

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
});
