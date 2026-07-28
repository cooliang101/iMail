import { describe, expect, it } from 'vitest';
import { authenticateToken, issueToken } from './tokens.js';
import type { StoreData } from './types.js';

function memoryStore(seed: StoreData = { accounts: [], messages: [], tokens: [] }) {
  let data = structuredClone(seed);
  return {
    read: async () => structuredClone(data),
    update: async (mutator: (value: StoreData) => void | Promise<void>) => { const next = structuredClone(data); await mutator(next); data = next; return structuredClone(data); },
  };
}

describe('developer tokens', () => {
  it('returns the raw token once while storing only its hash', async () => {
    const store = memoryStore();
    const result = await issueToken({ name: 'Local tests', scopes: ['messages:read'], accountIds: ['account-1'], ttlSeconds: 3600 }, store);
    expect(result.raw).toMatch(/^imail_[A-Za-z0-9_-]{40}$/);
    expect((await store.read()).tokens[0].tokenHash).not.toContain(result.raw);
    expect((await store.read()).tokens[0].prefix).toBe(result.raw.slice(0, 12));
  });

  it('authenticates permitted scopes and records last use', async () => {
    const store = memoryStore();
    const { raw } = await issueToken({ name: 'Local tests', scopes: ['messages:read'], accountIds: ['account-1'], ttlSeconds: 3600 }, store);
    const authenticated = await authenticateToken(raw, 'messages:read', store);
    expect(authenticated?.accountIds).toEqual(['account-1']);
    expect((await store.read()).tokens[0].lastUsedAt).toBeTruthy();
  });

  it('rejects malformed, unknown, expired and under-scoped tokens', async () => {
    const store = memoryStore();
    const { raw } = await issueToken({ name: 'Local tests', scopes: ['messages:read'], accountIds: ['account-1'], ttlSeconds: -1 }, store);
    await expect(authenticateToken(undefined, 'messages:read', store)).resolves.toBeNull();
    await expect(authenticateToken('wrong', 'messages:read', store)).resolves.toBeNull();
    await expect(authenticateToken('imail_unknown', 'messages:read', store)).resolves.toBeNull();
    await expect(authenticateToken(raw, 'messages:read', store)).resolves.toBeNull();
    const active = memoryStore();
    const issued = await issueToken({ name: 'Scoped', scopes: ['messages:read'], accountIds: ['account-1'], ttlSeconds: 3600 }, active);
    await expect(authenticateToken(issued.raw, 'messages:send', active)).resolves.toBeNull();
  });
});
