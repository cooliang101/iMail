import { createHash } from 'node:crypto';
import { ImapFlow, type FetchMessageObject } from 'imapflow';
import { simpleParser } from 'mailparser';
import nodemailer from 'nodemailer';
import { resolveAccountSecret } from './oauth.js';
import { readStore, updateStore } from './store.js';
import type { CachedMessage, MailAccount } from './types.js';

function authFor(account: MailAccount, secret: Awaited<ReturnType<typeof resolveAccountSecret>>) {
  return secret.accessToken
    ? { user: account.email, accessToken: secret.accessToken }
    : { user: account.email, pass: secret.password ?? '' };
}

function address(value?: { name?: string; address?: string } | null) {
  return { name: value?.name ?? '', address: value?.address ?? '' };
}

function smtpTransport(account: MailAccount, secret: Awaited<ReturnType<typeof resolveAccountSecret>>) {
  const auth = secret.accessToken
    ? { type: 'OAuth2' as const, user: account.email, accessToken: secret.accessToken }
    : { user: account.email, pass: secret.password ?? '' };
  const yahooOAuthBearer = secret.oauthProvider === 'yahoo' && secret.accessToken;
  return nodemailer.createTransport({
    host: account.settings.smtpHost,
    port: account.settings.smtpPort,
    secure: account.settings.smtpSecure,
    auth: yahooOAuthBearer ? { user: account.email, pass: 'oauth', method: 'OAUTHBEARER' } : auth,
    authMethod: yahooOAuthBearer ? 'OAUTHBEARER' : undefined,
    customAuth: yahooOAuthBearer ? {
      OAUTHBEARER: async (context) => {
        const payload = [
          `n,a=${account.email},`,
          `host=${account.settings.smtpHost}`,
          `port=${account.settings.smtpPort}`,
          `auth=Bearer ${secret.accessToken}`,
          '',
          '',
        ].join('\x01');
        const response = await context.sendCommand(`AUTH OAUTHBEARER ${Buffer.from(payload).toString('base64')}`);
        if (response.status !== 235) throw new Error(`Yahoo SMTP OAuth 验证失败 (${response.status})`);
        return true;
      },
    } : undefined,
    disableFileAccess: true,
    disableUrlAccess: true,
  });
}

type ProtocolError = Error & {
  responseText?: unknown;
  response?: unknown;
  responseStatus?: unknown;
  serverResponseCode?: unknown;
  code?: unknown;
};

export function describeProtocolError(stage: 'IMAP' | 'SMTP', error: unknown): Error {
  const value = error instanceof Error ? error as ProtocolError : undefined;
  const responseText = typeof value?.responseText === 'string' ? value.responseText : undefined;
  const response = typeof value?.response === 'string' ? value.response : undefined;
  const generic = value?.message === 'Command failed' ? undefined : value?.message;
  const detail = responseText || response || generic || '服务商拒绝了连接请求';
  const status = [value?.serverResponseCode, value?.responseStatus, value?.code]
    .find((item) => typeof item === 'string' || typeof item === 'number');
  const safeDetail = String(detail).replace(/[\r\n]+/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 500);
  return new Error(`${stage} 验证失败${status ? ` (${String(status)})` : ''}：${safeDetail}`);
}

export async function testAccount(account: MailAccount): Promise<void> {
  const secret = await resolveAccountSecret(account);
  const client = new ImapFlow({
    host: account.settings.imapHost,
    port: account.settings.imapPort,
    secure: account.settings.imapSecure,
    auth: authFor(account, secret),
    logger: false,
  });
  try {
    try {
      await client.connect();
      await client.mailboxOpen('INBOX', { readOnly: true });
    } catch (error) {
      throw describeProtocolError('IMAP', error);
    }
  } finally {
    await client.logout().catch(() => undefined);
  }
  const transport = smtpTransport(account, secret);
  try {
    try { await transport.verify(); }
    catch (error) { throw describeProtocolError('SMTP', error); }
  } finally { transport.close(); }
}

export async function syncAccount(accountId: string): Promise<{ synced: number }> {
  const store = await readStore();
  const account = store.accounts.find((item) => item.id === accountId);
  if (!account) throw new Error('邮箱账户不存在');
  await updateStore((data) => {
    const current = data.accounts.find((item) => item.id === accountId);
    if (current) { current.status = 'syncing'; current.lastError = undefined; }
  });

  const secret = await resolveAccountSecret(account);
  const client = new ImapFlow({
    host: account.settings.imapHost,
    port: account.settings.imapPort,
    secure: account.settings.imapSecure,
    auth: authFor(account, secret),
    logger: false,
  });

  try {
    await client.connect();
    const mailbox = await client.mailboxOpen('INBOX', { readOnly: true });
    const cached = store.messages.filter((message) => message.accountId === accountId && message.mailbox === 'INBOX');
    const maxCachedUid = cached.reduce((max, message) => Math.max(max, message.uid), 0);
    const incoming: CachedMessage[] = [];
    const flagUpdates = new Map<number, { unread: boolean; flagged: boolean }>();
    const parseIncoming = async (items: AsyncIterable<FetchMessageObject>) => {
      for await (const item of items) {
        if (!item.source) continue;
        const parsed = await simpleParser(item.source);
        const fromValue = parsed.from?.value[0];
        const toValue = parsed.to && !Array.isArray(parsed.to) ? parsed.to.value : Array.isArray(parsed.to) ? parsed.to.flatMap((entry) => entry.value) : [];
        const text = parsed.text?.trim() ?? '';
        const html = typeof parsed.html === 'string' ? parsed.html : undefined;
        incoming.push({
          id: createHash('sha256').update(`${accountId}:${item.uid}`).digest('hex').slice(0, 24),
          accountId,
          mailbox: 'INBOX',
          uid: item.uid,
          messageId: parsed.messageId,
          from: address(fromValue),
          to: toValue.map(address),
          subject: parsed.subject?.trim() || '（无主题）',
          preview: text.replace(/\s+/g, ' ').slice(0, 180),
          text,
          html,
          date: new Date(parsed.date ?? item.internalDate ?? Date.now()).toISOString(),
          unread: !item.flags?.has('\\Seen'),
          flagged: Boolean(item.flags?.has('\\Flagged')),
          hasAttachments: parsed.attachments.length > 0,
          attachments: parsed.attachments.map((attachment) => ({
            filename: attachment.filename ?? 'attachment',
            contentType: attachment.contentType,
            size: attachment.size,
          })),
        });
      }
    };

    if (mailbox.exists > 0 && maxCachedUid === 0) {
      const start = Math.max(1, mailbox.exists - 79);
      await parseIncoming(client.fetch(`${start}:*`, { uid: true, flags: true, source: true, envelope: true, internalDate: true }));
    } else if (maxCachedUid > 0 && mailbox.uidNext > maxCachedUid + 1) {
      await parseIncoming(client.fetch(`${maxCachedUid + 1}:*`, { uid: true, flags: true, source: true, envelope: true, internalDate: true }, { uid: true }));
    }

    const recentCachedUids = cached.sort((a, b) => b.uid - a.uid).slice(0, 100).map((message) => message.uid);
    if (recentCachedUids.length > 0) {
      for await (const item of client.fetch(recentCachedUids, { uid: true, flags: true }, { uid: true })) {
        flagUpdates.set(item.uid, { unread: !item.flags?.has('\\Seen'), flagged: Boolean(item.flags?.has('\\Flagged')) });
      }
    }

    await updateStore((data) => {
      const ids = new Set(incoming.map((message) => message.id));
      data.messages = [...data.messages.filter((message) => message.accountId !== accountId || !ids.has(message.id)), ...incoming]
        .sort((a, b) => b.date.localeCompare(a.date))
        .slice(0, 5000);
      for (const message of data.messages) {
        if (message.accountId !== accountId || message.mailbox !== 'INBOX') continue;
        const flags = flagUpdates.get(message.uid);
        if (flags) Object.assign(message, flags);
      }
      const current = data.accounts.find((item) => item.id === accountId);
      if (current) {
        current.status = 'connected';
        current.lastSyncAt = new Date().toISOString();
        current.lastError = undefined;
      }
    });
    return { synced: incoming.length };
  } catch (error) {
    const message = error instanceof Error ? error.message : '同步失败';
    await updateStore((data) => {
      const current = data.accounts.find((item) => item.id === accountId);
      if (current) { current.status = 'error'; current.lastError = message; }
    });
    throw error;
  } finally {
    await client.logout().catch(() => undefined);
  }
}

export async function updateRemoteMessageFlags(messageId: string, input: { unread?: boolean; flagged?: boolean }): Promise<void> {
  const store = await readStore();
  const message = store.messages.find((item) => item.id === messageId);
  if (!message) throw new Error('邮件不存在');
  const account = store.accounts.find((item) => item.id === message.accountId);
  if (!account) throw new Error('邮箱账户不存在');

  const secret = await resolveAccountSecret(account);
  const client = new ImapFlow({
    host: account.settings.imapHost,
    port: account.settings.imapPort,
    secure: account.settings.imapSecure,
    auth: authFor(account, secret),
    logger: false,
  });
  try {
    await client.connect();
    await client.mailboxOpen(message.mailbox, { readOnly: false });
    if (input.unread !== undefined) {
      if (input.unread) await client.messageFlagsRemove(message.uid, ['\\Seen'], { uid: true });
      else await client.messageFlagsAdd(message.uid, ['\\Seen'], { uid: true });
    }
    if (input.flagged !== undefined) {
      if (input.flagged) await client.messageFlagsAdd(message.uid, ['\\Flagged'], { uid: true });
      else await client.messageFlagsRemove(message.uid, ['\\Flagged'], { uid: true });
    }
  } catch (error) {
    throw describeProtocolError('IMAP', error);
  } finally {
    await client.logout().catch(() => undefined);
  }
}

export async function sendMessage(input: {
  accountId: string;
  to: string[];
  cc?: string[];
  subject: string;
  text: string;
  html?: string;
}) {
  const store = await readStore();
  const account = store.accounts.find((item) => item.id === input.accountId);
  if (!account) throw new Error('发件邮箱不存在');
  const secret = await resolveAccountSecret(account);
  const transport = smtpTransport(account, secret);
  const result = await transport.sendMail({
    from: { name: account.displayName, address: account.email },
    to: input.to,
    cc: input.cc,
    subject: input.subject,
    text: input.text,
    html: input.html,
  });
  return { messageId: result.messageId, accepted: result.accepted };
}
