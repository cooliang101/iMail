import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { MailAccount, StoreData } from '../types.js';

const state = vi.hoisted(() => ({
  data: { accounts: [], messages: [], tokens: [] } as StoreData,
  tested: undefined as MailAccount | undefined,
}));

vi.mock('../store.js', () => ({
  readStore: vi.fn(async () => state.data),
  updateStore: vi.fn(async (updater: (data: StoreData) => unknown) => updater(state.data)),
}));
vi.mock('../crypto.js', () => ({
  decryptSecret: vi.fn(async (value: string) => JSON.parse(value)),
  encryptSecret: vi.fn(async (value: unknown) => JSON.stringify(value)),
}));
vi.mock('../mail.js', () => ({
  testAccount: vi.fn(async (account: MailAccount) => { state.tested = account; }),
}));
vi.mock('../sync/store.js', () => ({ getSyncStore: () => ({ ensurePolicy: vi.fn() }) }));

import { updateAccountProxy } from './accounts.js';

function account(id: string, email: string, overrides: Partial<MailAccount> = {}): MailAccount {
  return {
    id, provider: 'gmail', email, displayName: email, group: '个人', color: '#168f78',
    settings: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
    encryptedSecret: JSON.stringify({ authType: 'app-password', password: `${id}-mail-password` }),
    authMethod: 'app-password', createdAt: '2026-08-05T00:00:00.000Z', status: 'connected', ...overrides,
  };
}

describe('reusable account proxy configuration', () => {
  beforeEach(() => { state.data = { accounts: [], messages: [], tokens: [] }; state.tested = undefined; });

  it('copies another account proxy and encrypted password without exposing it to the caller', async () => {
    const source = account('11111111-1111-4111-8111-111111111111', 'source@example.com', {
      proxy: { protocol: 'socks5', host: 'proxy.example.com', port: 1080, username: 'proxy-user' },
      encryptedSecret: JSON.stringify({ authType: 'app-password', password: 'source-mail-password', proxyPassword: 'source-proxy-password' }),
    });
    const target = account('22222222-2222-4222-8222-222222222222', 'target@example.com');
    state.data.accounts = [source, target];

    const updated = await updateAccountProxy(target.id, { enabled: true, sourceAccountId: source.id });

    expect(updated.proxy).toEqual(source.proxy);
    expect(state.tested?.proxy).toEqual(source.proxy);
    expect(JSON.parse(updated.encryptedSecret)).toMatchObject({ password: '22222222-2222-4222-8222-222222222222-mail-password', proxyPassword: 'source-proxy-password' });
    expect(source.encryptedSecret).toContain('source-proxy-password');
  });

  it('rejects an account that no longer has a reusable proxy', async () => {
    const source = account('11111111-1111-4111-8111-111111111111', 'source@example.com');
    const target = account('22222222-2222-4222-8222-222222222222', 'target@example.com');
    state.data.accounts = [source, target];
    await expect(updateAccountProxy(target.id, { enabled: true, sourceAccountId: source.id })).rejects.toThrow('没有可复用的代理配置');
  });
});
