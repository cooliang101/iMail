import { describe, expect, it } from 'vitest';
import { PROVIDERS, settingsFor } from './providers.js';
import { hashToken } from './tokens.js';

describe('provider presets', () => {
  it('uses TLS IMAP for every built-in provider', () => {
    for (const settings of Object.values(PROVIDERS)) {
      expect(settings.imapPort).toBe(993);
      expect(settings.imapSecure).toBe(true);
    }
  });

  it('requires settings for a custom provider', () => {
    expect(() => settingsFor('custom')).toThrow('完整的 IMAP/SMTP 配置');
  });
});

describe('developer token hashing', () => {
  it('is deterministic without storing the raw token', () => {
    expect(hashToken('rbx_example')).toBe(hashToken('rbx_example'));
    expect(hashToken('rbx_example')).not.toContain('rbx_example');
  });
});
