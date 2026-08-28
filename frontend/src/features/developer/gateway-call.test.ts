import { describe, expect, it } from 'vitest';
import { buildAgentGatewayContext, buildGatewayMessageUrl, resolveActiveApiCredential } from './gateway-call';
import type { ApiGatewayCredential } from '../../app-model';

const credential: ApiGatewayCredential = {
  rawToken: 'imail_secret',
  detail: { id: 'token-1', name: 'Agent', prefix: 'imail_', scopes: ['messages:read'], mailboxes: ['owner@icloud.com'], createdAt: '2026-08-28T00:00:00Z', expiresAt: '2026-08-28T01:00:00Z' },
};

describe('gateway call helpers', () => {
  it('encodes privacy mailbox addresses in gateway paths', () => {
    expect(buildGatewayMessageUrl('http://127.0.0.1:60052/gateway/v1', 'shop+private@icloud.com')).toBe(
      'http://127.0.0.1:60052/gateway/v1/mailboxes/shop%2Bprivate%40icloud.com/messages?limit=10',
    );
  });

  it('builds a complete agent handoff with the bearer token', () => {
    const context = buildAgentGatewayContext({
      endpoint: 'http://127.0.0.1:60052/gateway/v1',
      mailbox: 'private@icloud.com',
      rawToken: 'imail_secret',
      authorizedMailboxes: ['owner@icloud.com'],
    });
    expect(context).toContain('Authorization: Bearer imail_secret');
    expect(context).toContain('/mailboxes/private%40icloud.com/messages?limit=10');
    expect(context).toContain('Token 授权邮箱: owner@icloud.com');
  });

  it('rejects revoked and expired in-memory credentials', () => {
    expect(resolveActiveApiCredential(credential, [], Date.parse('2026-08-28T00:30:00Z'))).toBeUndefined();
    expect(resolveActiveApiCredential(credential, [credential.detail], Date.parse('2026-08-28T01:00:00Z'))).toBeUndefined();
    expect(resolveActiveApiCredential(credential, [credential.detail], Date.parse('2026-08-28T00:59:59Z'))).toBe(credential);
  });
});
