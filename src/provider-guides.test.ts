import { describe, expect, it } from 'vitest';
import { credentialGuideFor, oauthCallbackOrigins } from './provider-guides';

describe('provider credential guides', () => {
  it.each(['gmail', 'qq', 'yahoo', 'icloud'] as const)('provides a complete official fallback guide for %s', (provider) => {
    const guide = credentialGuideFor(provider);
    expect(guide?.helpUrl).toMatch(/^https:\/\//);
    expect(guide?.steps).toHaveLength(3);
    expect(guide?.secretLabel).toBeTruthy();
  });

  it.each(['outlook', 'hotmail'] as const)('does not offer obsolete basic authentication for %s', (provider) => {
    expect(credentialGuideFor(provider)).toBeUndefined();
  });

  it('derives trusted popup origins from configured OAuth redirect URIs', () => {
    expect([...oauthCallbackOrigins([
      'http://localhost:8787/api/oauth/google/callback',
      'https://mail.example.com/api/oauth/microsoft/callback',
      'not-a-url',
    ], 'https://app.example.com')]).toEqual([
      'https://app.example.com',
      'http://localhost:8787',
      'https://mail.example.com',
    ]);
  });
});
