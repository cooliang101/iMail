import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AccountSecret, MailAccount, StoreData } from './types.js';

const state = vi.hoisted(() => ({
  secret: { authType: 'app-password', password: 'app-password' } as AccountSecret,
  store: { accounts: [], messages: [], tokens: [] } as StoreData,
  imapOptions: [] as Array<Record<string, unknown>>,
  smtpOptions: [] as Array<Record<string, any>>,
  fetchItems: [] as Array<Record<string, any>>,
  parsed: {} as Record<string, any>,
  connect: vi.fn(async () => undefined),
  mailboxOpen: vi.fn(async () => ({ exists: 0 })),
  logout: vi.fn(async () => undefined),
  verify: vi.fn(async () => true),
  close: vi.fn(),
  sendMail: vi.fn(async () => ({ messageId: '<sent@example.com>', accepted: ['recipient@example.com'] })),
  simpleParser: vi.fn(async () => state.parsed),
}));

vi.mock('./oauth.js', () => ({ resolveAccountSecret: vi.fn(async () => state.secret) }));
vi.mock('./store.js', () => ({
  readStore: vi.fn(async () => structuredClone(state.store)),
  updateStore: vi.fn(async (mutator: (data: StoreData) => void | Promise<void>) => {
    const next = structuredClone(state.store); await mutator(next); state.store = next; return structuredClone(next);
  }),
}));
vi.mock('imapflow', () => ({
  ImapFlow: class {
    constructor(options: Record<string, unknown>) { state.imapOptions.push(options); }
    connect = state.connect;
    mailboxOpen = state.mailboxOpen;
    logout = state.logout;
    fetch() {
      const items = state.fetchItems;
      return { async *[Symbol.asyncIterator]() { for (const item of items) yield item; } };
    }
  },
}));
vi.mock('mailparser', () => ({ simpleParser: state.simpleParser }));
vi.mock('nodemailer', () => ({
  default: { createTransport: vi.fn((options: Record<string, any>) => {
    state.smtpOptions.push(options);
    return { verify: state.verify, close: state.close, sendMail: state.sendMail };
  }) },
}));

import { sendMessage, syncAccount, testAccount } from './mail.js';

function account(overrides: Partial<MailAccount> = {}): MailAccount {
  return {
    id: '11111111-1111-4111-8111-111111111111', provider: 'gmail', email: 'owner@example.com', displayName: 'Owner',
    group: '个人', color: '#168f78', settings: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
    encryptedSecret: 'ciphertext', authMethod: 'app-password', createdAt: '2026-07-28T00:00:00.000Z', status: 'connected', ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  state.secret = { authType: 'app-password', password: 'app-password' };
  state.store = { accounts: [], messages: [], tokens: [] };
  state.imapOptions = []; state.smtpOptions = []; state.fetchItems = [];
  state.connect.mockResolvedValue(undefined); state.mailboxOpen.mockResolvedValue({ exists: 0 }); state.logout.mockResolvedValue(undefined);
  state.verify.mockResolvedValue(true); state.sendMail.mockResolvedValue({ messageId: '<sent@example.com>', accepted: ['recipient@example.com'] });
  state.parsed = {};
});

describe('mail account connection', () => {
  it('verifies both IMAP and SMTP with an application password', async () => {
    await testAccount(account());
    expect(state.connect).toHaveBeenCalledOnce();
    expect(state.mailboxOpen).toHaveBeenCalledWith('INBOX', { readOnly: true });
    expect(state.imapOptions[0].auth).toEqual({ user: 'owner@example.com', pass: 'app-password' });
    expect(state.smtpOptions[0].auth).toEqual({ user: 'owner@example.com', pass: 'app-password' });
    expect(state.verify).toHaveBeenCalledOnce();
    expect(state.close).toHaveBeenCalledOnce();
  });

  it('uses OAuth access tokens for Gmail IMAP and SMTP', async () => {
    state.secret = { authType: 'oauth2', oauthProvider: 'google', accessToken: 'google-access' };
    await testAccount(account({ authMethod: 'oauth2' }));
    expect(state.imapOptions[0].auth).toEqual({ user: 'owner@example.com', accessToken: 'google-access' });
    expect(state.smtpOptions[0].auth).toEqual({ type: 'OAuth2', user: 'owner@example.com', accessToken: 'google-access' });
  });
});

describe('message synchronization', () => {
  it('maps IMAP messages, flags and attachment metadata into the cache', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.mailboxOpen.mockResolvedValue({ exists: 1 });
    state.fetchItems = [{ uid: 42, source: Buffer.from('mail'), flags: new Set(['\\Flagged']), internalDate: new Date('2026-07-28T01:00:00.000Z') }];
    state.parsed = {
      from: { value: [{ name: 'Sender', address: 'sender@example.com' }] }, to: { value: [{ name: 'Owner', address: 'owner@example.com' }] },
      subject: ' Hello ', text: ' Body text ', html: '<p>Body text</p>', messageId: '<42@example.com>', date: new Date('2026-07-28T01:00:00.000Z'),
      attachments: [{ filename: 'note.txt', contentType: 'text/plain', size: 12 }],
    };
    await expect(syncAccount(configured.id)).resolves.toEqual({ synced: 1 });
    expect(state.store.messages[0]).toMatchObject({ uid: 42, subject: 'Hello', text: 'Body text', unread: true, flagged: true, hasAttachments: true });
    expect(state.store.messages[0].attachments).toEqual([{ filename: 'note.txt', contentType: 'text/plain', size: 12 }]);
    expect(state.store.accounts[0].status).toBe('connected');
    expect(state.store.accounts[0].lastSyncAt).toBeTruthy();
  });

  it('records a connection error and always logs out', async () => {
    const configured = account(); state.store.accounts = [configured]; state.connect.mockRejectedValueOnce(new Error('IMAP unavailable'));
    await expect(syncAccount(configured.id)).rejects.toThrow('IMAP unavailable');
    expect(state.store.accounts[0]).toMatchObject({ status: 'error', lastError: 'IMAP unavailable' });
    expect(state.logout).toHaveBeenCalledOnce();
  });

  it('rejects unknown accounts before opening a connection', async () => {
    await expect(syncAccount('missing')).rejects.toThrow('邮箱账户不存在');
    expect(state.connect).not.toHaveBeenCalled();
  });
});

describe('message sending', () => {
  it('sends through Gmail OAuth with local file and URL access disabled', async () => {
    const configured = account({ authMethod: 'oauth2' }); state.store.accounts = [configured];
    state.secret = { authType: 'oauth2', oauthProvider: 'google', accessToken: 'google-access' };
    await expect(sendMessage({ accountId: configured.id, to: ['recipient@example.com'], subject: 'Hello', text: 'Body' })).resolves.toEqual({ messageId: '<sent@example.com>', accepted: ['recipient@example.com'] });
    expect(state.smtpOptions[0]).toMatchObject({ disableFileAccess: true, disableUrlAccess: true, auth: { type: 'OAuth2', accessToken: 'google-access' } });
    expect(state.sendMail).toHaveBeenCalledWith(expect.objectContaining({ from: { name: 'Owner', address: 'owner@example.com' }, to: ['recipient@example.com'] }));
  });

  it('constructs Yahoo OAUTHBEARER framing and validates the SMTP response', async () => {
    const configured = account({ provider: 'yahoo', settings: { imapHost: 'imap.mail.yahoo.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.mail.yahoo.com', smtpPort: 465, smtpSecure: true }, authMethod: 'oauth2' });
    state.store.accounts = [configured]; state.secret = { authType: 'oauth2', oauthProvider: 'yahoo', accessToken: 'yahoo-access' };
    await sendMessage({ accountId: configured.id, to: ['recipient@example.com'], subject: 'Hello', text: 'Body' });
    expect(state.smtpOptions[0].authMethod).toBe('OAUTHBEARER');
    const sendCommand = vi.fn(async (_command: string) => ({ status: 235 }));
    await expect(state.smtpOptions[0].customAuth.OAUTHBEARER({ sendCommand })).resolves.toBe(true);
    const encoded = String(sendCommand.mock.calls[0][0]).replace('AUTH OAUTHBEARER ', '');
    expect(Buffer.from(encoded, 'base64').toString()).toContain('auth=Bearer yahoo-access');
    await expect(state.smtpOptions[0].customAuth.OAUTHBEARER({ sendCommand: async () => ({ status: 535 }) })).rejects.toThrow('Yahoo SMTP OAuth 验证失败');
  });

  it('rejects an unknown sender account', async () => {
    await expect(sendMessage({ accountId: 'missing', to: ['recipient@example.com'], subject: 'Hello', text: 'Body' })).rejects.toThrow('发件邮箱不存在');
  });
});
