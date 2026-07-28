import { createHash } from 'node:crypto';
import { ImapFlow } from 'imapflow';
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
    await client.connect();
    await client.mailboxOpen('INBOX', { readOnly: true });
  } finally {
    await client.logout().catch(() => undefined);
  }
  const transport = smtpTransport(account, secret);
  try { await transport.verify(); } finally { transport.close(); }
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
    const start = Math.max(1, mailbox.exists - 79);
    const incoming: CachedMessage[] = [];
    if (mailbox.exists > 0) {
      for await (const item of client.fetch(`${start}:*`, { uid: true, flags: true, source: true, envelope: true, internalDate: true })) {
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
    }

    await updateStore((data) => {
      const ids = new Set(incoming.map((message) => message.id));
      data.messages = [...data.messages.filter((message) => message.accountId !== accountId || !ids.has(message.id)), ...incoming]
        .sort((a, b) => b.date.localeCompare(a.date))
        .slice(0, 5000);
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
