import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AccountSecret, MailAccount, StoreData } from './types.js';

const state = vi.hoisted(() => ({
  secret: { authType: 'app-password', password: 'app-password' } as AccountSecret,
  store: { accounts: [], messages: [], tokens: [] } as StoreData,
  imapOptions: [] as Array<Record<string, unknown>>,
  smtpOptions: [] as Array<Record<string, any>>,
  fetchBatches: [] as Array<Array<Record<string, any>>>,
  fetchCalls: [] as Array<{ range: unknown; query: Record<string, unknown>; options?: Record<string, unknown> }>,
  parsed: {} as Record<string, any>,
  connect: vi.fn(async () => undefined),
  mailboxOpen: vi.fn(async () => ({ exists: 0, uidNext: 1 })),
  logout: vi.fn(async () => undefined),
  verify: vi.fn(async () => true),
  close: vi.fn(),
  sendMail: vi.fn(async () => ({ messageId: '<sent@example.com>', accepted: ['recipient@example.com'] })),
  messageFlagsAdd: vi.fn(async () => true),
  messageFlagsRemove: vi.fn(async () => true),
  list: vi.fn(async () => [{ path: 'Sent', specialUse: '\\Sent' }, { path: 'Archive', specialUse: '\\Archive' }, { path: 'Trash', specialUse: '\\Trash' }]),
  messageMove: vi.fn(async () => ({ uidValidity: 1n, uidMap: new Map([[42, 84]]) })),
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
    messageFlagsAdd = state.messageFlagsAdd;
    messageFlagsRemove = state.messageFlagsRemove;
    list = state.list;
    messageMove = state.messageMove;
    fetch(range: unknown, query: Record<string, unknown>, options?: Record<string, unknown>) {
      const index = state.fetchCalls.length;
      state.fetchCalls.push({ range, query, options });
      const items = state.fetchBatches[index] ?? [];
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

import { describeProtocolError, downloadAttachment, moveRemoteMessage, sendMessage, syncAccount, testAccount, updateRemoteMessageFlags } from './mail.js';
import { gatewayEvents, type GatewayMessageCreatedEvent } from './gateway/events.js';

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
  state.imapOptions = []; state.smtpOptions = []; state.fetchBatches = []; state.fetchCalls = [];
  state.connect.mockResolvedValue(undefined); state.mailboxOpen.mockResolvedValue({ exists: 0, uidNext: 1 }); state.logout.mockResolvedValue(undefined);
  state.verify.mockResolvedValue(true); state.sendMail.mockResolvedValue({ messageId: '<sent@example.com>', accepted: ['recipient@example.com'] });
  state.messageFlagsAdd.mockResolvedValue(true); state.messageFlagsRemove.mockResolvedValue(true);
  state.list.mockResolvedValue([{ path: 'Sent', specialUse: '\\Sent' }, { path: 'Archive', specialUse: '\\Archive' }, { path: 'Trash', specialUse: '\\Trash' }]);
  state.messageMove.mockResolvedValue({ uidValidity: 1n, uidMap: new Map([[42, 84]]) });
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

  it('surfaces the provider response instead of a generic IMAP command error', async () => {
    const failure = Object.assign(new Error('Command failed'), { responseText: '[AUTHENTICATIONFAILED] Invalid credentials', responseStatus: 'NO' });
    state.connect.mockRejectedValueOnce(failure);
    await expect(testAccount(account({ authMethod: 'oauth2' }))).rejects.toThrow('IMAP 验证失败 (NO)：[AUTHENTICATIONFAILED] Invalid credentials');
    expect(describeProtocolError('IMAP', failure).message).not.toContain('Command failed');
  });

  it('identifies SMTP verification failures separately', async () => {
    state.verify.mockRejectedValueOnce(Object.assign(new Error('Invalid login'), { code: 'EAUTH', response: '535 Authentication failed' }));
    await expect(testAccount(account())).rejects.toThrow('SMTP 验证失败 (EAUTH)：535 Authentication failed');
  });
});

describe('message synchronization', () => {
  it('writes read and star changes back to the source IMAP mailbox by UID', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.store.messages = [{ id: 'cached-42', accountId: configured.id, mailbox: 'INBOX', uid: 42, from: { name: '', address: '' }, to: [], subject: 'Cached', preview: '', text: 'Body', date: '2026-07-28T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [] }];
    await expect(updateRemoteMessageFlags('cached-42', { unread: false, flagged: true })).resolves.toBeUndefined();
    expect(state.mailboxOpen).toHaveBeenCalledWith('INBOX', { readOnly: false });
    expect(state.messageFlagsAdd).toHaveBeenCalledWith(42, ['\\Seen'], { uid: true });
    expect(state.messageFlagsAdd).toHaveBeenCalledWith(42, ['\\Flagged'], { uid: true });
    expect(state.logout).toHaveBeenCalledOnce();
  });

  it('removes source IMAP flags when a message becomes unread or unstarred', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.store.messages = [{ id: 'cached-43', accountId: configured.id, mailbox: 'INBOX', uid: 43, from: { name: '', address: '' }, to: [], subject: 'Cached', preview: '', text: 'Body', date: '2026-07-28T00:00:00.000Z', unread: false, flagged: true, hasAttachments: false, attachments: [] }];
    await updateRemoteMessageFlags('cached-43', { unread: true, flagged: false });
    expect(state.messageFlagsRemove).toHaveBeenCalledWith(43, ['\\Seen'], { uid: true });
    expect(state.messageFlagsRemove).toHaveBeenCalledWith(43, ['\\Flagged'], { uid: true });
  });

  it('moves a cached message to the provider special-use archive folder', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.store.messages = [{ id: 'cached-42', accountId: configured.id, mailbox: 'INBOX', uid: 42, from: { name: '', address: '' }, to: [], subject: 'Cached', preview: '', text: 'Body', date: '2026-07-28T00:00:00.000Z', unread: false, flagged: false, hasAttachments: false, attachments: [] }];
    await expect(moveRemoteMessage('cached-42', 'archive')).resolves.toEqual({ mailbox: 'Archive', uid: 84 });
    expect(state.list).toHaveBeenCalledOnce();
    expect(state.mailboxOpen).toHaveBeenCalledWith('INBOX', { readOnly: false });
    expect(state.messageMove).toHaveBeenCalledWith(42, 'Archive', { uid: true });
  });

  it('rejects a move when the provider does not expose the requested special-use folder', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.store.messages = [{ id: 'cached-42', accountId: configured.id, mailbox: 'INBOX', uid: 42, from: { name: '', address: '' }, to: [], subject: 'Cached', preview: '', text: 'Body', date: '2026-07-28T00:00:00.000Z', unread: false, flagged: false, hasAttachments: false, attachments: [] }];
    state.list.mockResolvedValueOnce([{ path: 'INBOX', specialUse: '\\Inbox' }]);
    await expect(moveRemoteMessage('cached-42', 'trash')).rejects.toThrow('服务商没有返回垃圾箱文件夹');
    expect(state.messageMove).not.toHaveBeenCalled();
  });

  it('uses Gmail All Mail as the archive destination when no standard archive folder exists', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.store.messages = [{ id: 'gmail-archive', accountId: configured.id, mailbox: 'INBOX', uid: 42, from: { name: '', address: '' }, to: [], subject: 'Cached', preview: '', text: 'Body', date: '2026-07-28T00:00:00.000Z', unread: false, flagged: false, hasAttachments: false, attachments: [] }];
    state.list.mockResolvedValueOnce([{ path: '[Gmail]/All Mail', specialUse: '\\All' }]);
    await expect(moveRemoteMessage('gmail-archive', 'archive')).resolves.toEqual({ mailbox: '[Gmail]/All Mail', uid: 84 });
    expect(state.messageMove).toHaveBeenCalledWith(42, '[Gmail]/All Mail', { uid: true });
  });

  it('maps IMAP messages, flags and attachment metadata into the cache', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.mailboxOpen.mockResolvedValue({ exists: 1, uidNext: 43 });
    state.fetchBatches = [[{ uid: 42, source: Buffer.from('mail'), flags: new Set(['\\Flagged']), internalDate: new Date('2026-07-28T01:00:00.000Z') }]];
    state.parsed = {
      from: { value: [{ name: 'Sender', address: 'sender@example.com' }] }, to: { value: [{ name: 'Owner', address: 'owner@example.com' }] },
      subject: ' Hello ', text: ' Body text ', html: '<p>Body text</p>', messageId: '<42@example.com>', date: new Date('2026-07-28T01:00:00.000Z'),
      attachments: [{ filename: 'note.txt', contentType: 'text/plain', size: 12 }],
    };
    await expect(syncAccount(configured.id)).resolves.toEqual({ synced: 1 });
    expect(state.store.messages[0]).toMatchObject({ uid: 42, subject: 'Hello', text: 'Body text', unread: true, flagged: true, hasAttachments: true });
    expect(state.store.messages[0].attachments).toEqual([{ filename: 'note.txt', contentType: 'text/plain', size: 12, index: 0 }]);
    expect(state.store.accounts[0].status).toBe('connected');
    expect(state.store.accounts[0].lastSyncAt).toBeTruthy();
  });

  it('uses cached UIDs, downloads only new messages and refreshes recent flags without message bodies', async () => {
    const configured = account({ lastSyncAt: '2026-07-28T00:00:00.000Z' });
    state.store.accounts = [configured];
    state.store.messages = [{
      id: 'cached-42', accountId: configured.id, mailbox: 'INBOX', uid: 42,
      from: { name: 'Cached', address: 'cached@example.com' }, to: [], subject: 'Cached', preview: 'Cached', text: 'Cached body',
      date: '2026-07-28T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [],
    }];
    state.mailboxOpen.mockResolvedValue({ exists: 43, uidNext: 44 });
    state.fetchBatches = [
      [{ uid: 43, source: Buffer.from('new-mail'), flags: new Set(), internalDate: new Date('2026-07-29T01:00:00.000Z') }],
      [{ uid: 42, flags: new Set(['\\Seen', '\\Flagged']) }],
    ];
    state.parsed = { from: { value: [{ name: 'New', address: 'new@example.com' }] }, to: { value: [] }, subject: 'New', text: 'New body', attachments: [] };
    const events: GatewayMessageCreatedEvent[] = [];
    const unsubscribe = gatewayEvents.subscribe((event) => events.push(event));
    await expect(syncAccount(configured.id)).resolves.toEqual({ synced: 1 });
    unsubscribe();
    expect(state.fetchCalls[0]).toMatchObject({ range: '43:*', options: { uid: true }, query: { source: true } });
    expect(state.fetchCalls[1]).toMatchObject({ range: [42], options: { uid: true }, query: { flags: true } });
    expect(state.fetchCalls[1].query).not.toHaveProperty('source');
    expect(state.simpleParser).toHaveBeenCalledOnce();
    expect(state.store.messages.find((message) => message.uid === 42)).toMatchObject({ text: 'Cached body', unread: false, flagged: true });
    expect(state.store.messages.find((message) => message.uid === 43)).toMatchObject({ text: 'New body' });
    expect(events).toHaveLength(1);
    expect(events[0]).toMatchObject({ type: 'message.created', accountId: configured.id, data: { message: { subject: 'New', accountEmail: configured.email } } });
  });

  it('does not redownload cached bodies when the mailbox has no new UID', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.store.messages = [{ id: 'cached-42', accountId: configured.id, mailbox: 'INBOX', uid: 42, from: { name: '', address: '' }, to: [], subject: 'Cached', preview: '', text: 'Body', date: '2026-07-28T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [] }];
    state.mailboxOpen.mockResolvedValue({ exists: 42, uidNext: 43 });
    state.fetchBatches = [[{ uid: 42, flags: new Set() }]];
    await expect(syncAccount(configured.id)).resolves.toEqual({ synced: 0 });
    expect(state.fetchCalls).toHaveLength(1);
    expect(state.fetchCalls[0].query).not.toHaveProperty('source');
    expect(state.simpleParser).not.toHaveBeenCalled();
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

  it('discovers and caches the provider sent folder separately from inbox', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.mailboxOpen.mockResolvedValue({ exists: 1, uidNext: 8 });
    state.fetchBatches = [[{ uid: 7, source: Buffer.from('sent-mail'), flags: new Set(['\\Seen']), internalDate: new Date('2026-07-29T02:00:00.000Z') }]];
    state.parsed = { from: { value: [{ name: 'Owner', address: configured.email }] }, to: { value: [{ name: '', address: 'friend@example.com' }] }, subject: 'Sent subject', text: 'Sent body', attachments: [] };
    await expect(syncAccount(configured.id, 'sent')).resolves.toEqual({ synced: 1 });
    expect(state.list).toHaveBeenCalledOnce();
    expect(state.mailboxOpen).toHaveBeenCalledWith('Sent', { readOnly: true });
    expect(state.store.messages[0]).toMatchObject({ mailbox: 'Sent', mailboxRole: 'sent', uid: 7, unread: false });
  });

  it('persists provider folders and syncs a selectable custom folder by path', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.list.mockResolvedValueOnce([
      { path: 'INBOX', name: 'INBOX', delimiter: '/', flags: new Set(), listed: true, subscribed: true, specialUse: '\\Inbox', status: { messages: 12, unseen: 3 } },
      { path: 'Projects/Alpha', name: 'Alpha', delimiter: '/', flags: new Set(), listed: true, subscribed: true, status: { messages: 1, unseen: 1 } },
    ] as any);
    state.mailboxOpen.mockResolvedValue({ exists: 1, uidNext: 10 });
    state.fetchBatches = [[{ uid: 9, source: Buffer.from('project-mail'), flags: new Set(), internalDate: new Date('2026-07-29T03:00:00.000Z') }]];
    state.parsed = { from: { value: [{ name: 'Teammate', address: 'team@example.com' }] }, to: { value: [] }, subject: 'Project update', text: 'Custom folder body', attachments: [] };

    await expect(syncAccount(configured.id, 'custom', 'Projects/Alpha')).resolves.toEqual({ synced: 1 });

    expect(state.mailboxOpen).toHaveBeenCalledWith('Projects/Alpha', { readOnly: true });
    expect(state.store.messages[0]).toMatchObject({ mailbox: 'Projects/Alpha', mailboxRole: 'custom', uid: 9 });
    expect(state.store.accounts[0].mailboxes).toEqual(expect.arrayContaining([
      expect.objectContaining({ path: 'Projects/Alpha', name: 'Alpha', selectable: true, total: 1, unread: 1 }),
    ]));
  });

  it('downloads attachment bytes from the cached message mailbox only on demand', async () => {
    const configured = account(); state.store.accounts = [configured];
    state.store.messages = [{ id: 'with-attachment', accountId: configured.id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: 44, from: { name: '', address: '' }, to: [], subject: 'File', preview: '', text: 'Body', date: '2026-07-28T00:00:00.000Z', unread: false, flagged: false, hasAttachments: true, attachments: [{ filename: 'report.txt', contentType: 'text/plain', size: 5, index: 0 }], labels: [] }];
    state.fetchBatches = [[{ uid: 44, source: Buffer.from('mail-source') }]];
    state.parsed = { attachments: [{ filename: 'report.txt', contentType: 'text/plain', size: 5, content: Buffer.from('hello') }] };
    await expect(downloadAttachment('with-attachment', 0)).resolves.toEqual({ content: Buffer.from('hello'), filename: 'report.txt', contentType: 'text/plain' });
    expect(state.mailboxOpen).toHaveBeenCalledWith('INBOX', { readOnly: true });
    expect(state.fetchCalls[0]).toMatchObject({ range: 44, options: { uid: true }, query: { source: true } });
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

  it('passes rich HTML and in-memory attachments to Nodemailer', async () => {
    const configured = account();
    state.store.accounts = [configured];
    await sendMessage({ accountId: configured.id, to: ['recipient@example.com'], subject: 'Rich mail', text: 'Body', html: '<p><strong>Body</strong></p>', attachments: [{ filename: 'note.txt', contentType: 'text/plain', data: 'aGVsbG8=' }] });
    expect(state.sendMail).toHaveBeenCalledWith(expect.objectContaining({
      html: '<p><strong>Body</strong></p>',
      attachDataUrls: true,
      attachments: [expect.objectContaining({ filename: 'note.txt', contentType: 'text/plain', content: Buffer.from('hello') })],
    }));
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
