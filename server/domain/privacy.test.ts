import { createDecipheriv, scrypt } from 'node:crypto';
import { describe, expect, it, vi } from 'vitest';
import type { MailAccount, StoreData } from '../types.js';

type DecryptBarrier = { started: () => void; released: Promise<void> };
const testState = vi.hoisted(() => ({
  userId: 'owner-user' as string | undefined,
  data: { accounts: [], messages: [], tokens: [], drafts: [], contacts: [], logoFetchAttempts: [] } as StoreData,
  decryptBarrier: undefined as DecryptBarrier | undefined,
}));

vi.mock('../crypto.js', () => ({
  decryptSecret: vi.fn(async (payload: string) => {
    const barrier = testState.decryptBarrier;
    if (barrier) { barrier.started(); await barrier.released; }
    return JSON.parse(payload);
  }),
}));
vi.mock('../auth/context.js', () => ({ currentUserId: () => testState.userId }));
vi.mock('../store.js', () => ({
  readStore: vi.fn(async () => structuredClone(testState.data)),
  updateStore: vi.fn(async (mutator: (data: StoreData) => void | Promise<void>) => {
    await mutator(testState.data);
    return structuredClone(testState.data);
  }),
}));

import {
  clearCurrentUserMailData,
  consumeMailAuthorizationExport,
  discardUserMailAuthorizationExports,
  MAIL_AUTHORIZATION_EXPORT_FORMAT,
  MAIL_AUTHORIZATION_EXPORT_VERSION,
  prepareMailAuthorizationExport,
  privacyInternals,
  type MailAuthorizationExportEnvelope,
} from './privacy.js';

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

function resetStore(accounts: MailAccount[] = []) {
  testState.data = { accounts, messages: [], tokens: [], drafts: [], contacts: [], logoFetchAttempts: [] };
}

function account(overrides: Partial<MailAccount> = {}): MailAccount {
  return {
    id: '11111111-1111-4111-8111-111111111111',
    provider: 'gmail',
    email: 'owner@example.com',
    displayName: 'Owner',
    group: '个人',
    groupIcon: 'users',
    color: '#168f78',
    settings: {
      imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true,
      smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true,
    },
    proxy: { protocol: 'https', host: 'proxy.example.com', port: 8443, username: 'proxy-user' },
    encryptedSecret: JSON.stringify({ authType: 'oauth2', oauthProvider: 'google', accessToken: 'access-secret', refreshToken: 'refresh-secret', proxyPassword: 'proxy-secret' }),
    authMethod: 'oauth2',
    createdAt: '2026-08-04T00:00:00.000Z',
    lastSyncAt: '2026-08-04T01:00:00.000Z',
    status: 'error',
    lastError: 'must-not-export-error',
    mailboxes: [{ path: 'INBOX', name: 'Inbox', delimiter: '/', selectable: true, subscribed: true }],
    ...overrides,
  };
}

async function decrypt(envelope: MailAuthorizationExportEnvelope, password: string) {
  const key = await new Promise<Buffer>((resolve, reject) => {
    scrypt(password, Buffer.from(envelope.kdf.salt, 'base64url'), envelope.kdf.keyLength, {
      N: envelope.kdf.cost, r: envelope.kdf.blockSize, p: envelope.kdf.parallelization, maxmem: 64 * 1024 * 1024,
    }, (error, output) => error ? reject(error) : resolve(output));
  });
  try {
    const decipher = createDecipheriv('aes-256-gcm', key, Buffer.from(envelope.cipher.iv, 'base64url'));
    decipher.setAAD(Buffer.from(`${envelope.format}:v${envelope.formatVersion}`));
    decipher.setAuthTag(Buffer.from(envelope.cipher.authTag, 'base64url'));
    return JSON.parse(Buffer.concat([decipher.update(Buffer.from(envelope.ciphertext, 'base64url')), decipher.final()]).toString('utf8'));
  } finally { key.fill(0); }
}

describe('mail authorization export', () => {
  it('refuses a caller-supplied user id without the matching async user context', async () => {
    await expect(prepareMailAuthorizationExport('foreign-user', 'portable-export-password')).rejects.toThrow('匹配的用户上下文');
    await expect(clearCurrentUserMailData('foreign-user')).rejects.toThrow('匹配的用户上下文');
  });

  it('selects only portable account configuration and decrypted authorization fields', async () => {
    const payload = await privacyInternals.buildPayload([account()]);
    expect(payload).toMatchObject({
      format: MAIL_AUTHORIZATION_EXPORT_FORMAT,
      formatVersion: MAIL_AUTHORIZATION_EXPORT_VERSION,
      accounts: [{
        provider: 'gmail', email: 'owner@example.com', displayName: 'Owner', group: '个人', groupIcon: 'users', color: '#168f78', authMethod: 'oauth2',
        settings: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
        proxy: { protocol: 'https', host: 'proxy.example.com', port: 8443, username: 'proxy-user' },
        authorization: { authType: 'oauth2', oauthProvider: 'google', accessToken: 'access-secret', refreshToken: 'refresh-secret', proxyPassword: 'proxy-secret' },
      }],
    });
    const exported = payload.accounts[0] as unknown as Record<string, unknown>;
    for (const key of ['id', 'ownerId', 'encryptedSecret', 'createdAt', 'lastSyncAt', 'status', 'lastError', 'mailboxes']) expect(exported).not.toHaveProperty(key);
    expect(JSON.stringify(payload)).not.toContain('must-not-export-error');
  });

  it('uses independent scrypt and authenticated encryption with unique salts and IVs', async () => {
    const payload = await privacyInternals.buildPayload([account()]);
    const password = 'portable-export-password';
    const first = await privacyInternals.encryptPayload(payload, password);
    const second = await privacyInternals.encryptPayload(payload, password);
    expect(first).toMatchObject({
      format: MAIL_AUTHORIZATION_EXPORT_FORMAT, formatVersion: MAIL_AUTHORIZATION_EXPORT_VERSION,
      kdf: { algorithm: 'scrypt', cost: 32768, blockSize: 8, parallelization: 1, keyLength: 32 },
      cipher: { algorithm: 'aes-256-gcm' }, ciphertext: expect.any(String),
    });
    expect(first.kdf.salt).not.toBe(second.kdf.salt);
    expect(first.cipher.iv).not.toBe(second.cipher.iv);
    expect(JSON.stringify(first)).not.toContain('access-secret');
    await expect(decrypt(first, password)).resolves.toEqual(payload);
    await expect(decrypt(first, 'wrong-export-password')).rejects.toThrow();
    const tampered = { ...first, ciphertext: `${first.ciphertext.slice(0, -2)}aa` };
    await expect(decrypt(tampered, password)).rejects.toThrow();
  });

  it('serializes clearing behind an in-flight export and invalidates the prepared credentials', async () => {
    resetStore([account()]);
    const started = deferred();
    const released = deferred();
    testState.decryptBarrier = { started: started.resolve, released: released.promise };
    const preparing = prepareMailAuthorizationExport('owner-user', 'portable-export-password');
    await started.promise;

    const clearing = clearCurrentUserMailData('owner-user');
    await Promise.resolve();
    expect(testState.data.accounts).toHaveLength(1);

    released.resolve();
    const prepared = await preparing;
    await expect(clearing).resolves.toEqual({ accountCount: 1 });
    expect(testState.data.accounts).toEqual([]);
    expect(consumeMailAuthorizationExport(prepared.id, 'owner-user')).toBeUndefined();
    testState.decryptBarrier = undefined;
  });

  it('keeps only the latest result when two exports are prepared concurrently', async () => {
    resetStore([account()]);
    const started = deferred();
    const released = deferred();
    testState.decryptBarrier = { started: started.resolve, released: released.promise };
    const firstPromise = prepareMailAuthorizationExport('owner-user', 'first-portable-password');
    await started.promise;
    const secondPromise = prepareMailAuthorizationExport('owner-user', 'second-portable-password');

    released.resolve();
    const first = await firstPromise;
    testState.decryptBarrier = undefined;
    const second = await secondPromise;
    expect(consumeMailAuthorizationExport(first.id, 'owner-user')).toBeUndefined();
    const latest = consumeMailAuthorizationExport(second.id, 'owner-user');
    expect(latest).toBeDefined();
    latest?.body.fill(0);
    discardUserMailAuthorizationExports('owner-user');
  });
});
