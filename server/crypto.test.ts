import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';

const directories: string[] = [];

afterEach(async () => {
  delete process.env.APP_MASTER_KEY;
  delete process.env.IMAIL_DATA_DIR;
  vi.resetModules();
  await Promise.all(directories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

async function isolatedCrypto() {
  const directory = await mkdtemp(path.join(tmpdir(), 'imail-crypto-'));
  directories.push(directory); process.env.IMAIL_DATA_DIR = directory; vi.resetModules();
  return import('./crypto.js');
}

describe('credential encryption', () => {
  it('encrypts and decrypts OAuth credentials without exposing plaintext', async () => {
    const { encryptSecret, decryptSecret } = await isolatedCrypto();
    const secret = { authType: 'oauth2' as const, accessToken: 'access-secret', refreshToken: 'refresh-secret', expiresAt: '2026-07-29T00:00:00.000Z', oauthProvider: 'google' as const };
    const encrypted = await encryptSecret(secret);
    expect(encrypted).not.toContain('access-secret');
    await expect(decryptSecret(encrypted)).resolves.toEqual(secret);
  });

  it('uses unique IVs for repeated encryption', async () => {
    const { encryptSecret } = await isolatedCrypto();
    expect(await encryptSecret({ password: 'same' })).not.toBe(await encryptSecret({ password: 'same' }));
  });

  it('rejects tampered ciphertext', async () => {
    const { encryptSecret, decryptSecret } = await isolatedCrypto();
    const encrypted = await encryptSecret({ password: 'secret' });
    await expect(decryptSecret(`${encrypted.slice(0, -2)}aa`)).rejects.toThrow();
  });

  it('validates configured master key length', async () => {
    process.env.APP_MASTER_KEY = 'abcd';
    const { encryptSecret } = await isolatedCrypto();
    await expect(encryptSecret({ password: 'secret' })).rejects.toThrow('32 字节');
  });
});
