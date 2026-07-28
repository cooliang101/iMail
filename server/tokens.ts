import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';
import { readStore, updateStore } from './store.js';
import type { DeveloperToken, TokenScope } from './types.js';

export function hashToken(token: string): string {
  return createHash('sha256').update(token).digest('hex');
}

export async function issueToken(input: { name: string; scopes: TokenScope[]; accountIds: string[]; ttlSeconds: number }) {
  const raw = `imail_${randomBytes(30).toString('base64url')}`;
  const now = new Date();
  const token: DeveloperToken = {
    id: crypto.randomUUID(),
    name: input.name,
    tokenHash: hashToken(raw),
    prefix: raw.slice(0, 12),
    scopes: input.scopes,
    accountIds: input.accountIds,
    createdAt: now.toISOString(),
    expiresAt: new Date(now.getTime() + input.ttlSeconds * 1000).toISOString(),
  };
  await updateStore((data) => { data.tokens.push(token); });
  return { token, raw };
}

export async function authenticateToken(raw: string | undefined, scope: TokenScope): Promise<DeveloperToken | null> {
  if (!raw?.startsWith('imail_')) return null;
  const digest = Buffer.from(hashToken(raw), 'hex');
  const store = await readStore();
  const token = store.tokens.find((candidate) => {
    const candidateDigest = Buffer.from(candidate.tokenHash, 'hex');
    return candidateDigest.length === digest.length && timingSafeEqual(candidateDigest, digest);
  });
  if (!token || token.expiresAt <= new Date().toISOString() || !token.scopes.includes(scope)) return null;
  await updateStore((data) => {
    const current = data.tokens.find((item) => item.id === token.id);
    if (current) current.lastUsedAt = new Date().toISOString();
  });
  return token;
}
