import { describe, expect, it } from 'vitest';
import { OAUTH_WAIT_TIMEOUT_MS, oauthWaitExpired } from './oauth-flow';

describe('OAuth flow timing', () => {
  it('expires only after the shared timeout', () => {
    expect(oauthWaitExpired(1_000, 1_000 + OAUTH_WAIT_TIMEOUT_MS)).toBe(false);
    expect(oauthWaitExpired(1_000, 1_001 + OAUTH_WAIT_TIMEOUT_MS)).toBe(true);
  });
});
