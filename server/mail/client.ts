import { ImapFlow } from 'imapflow';
import nodemailer from 'nodemailer';
import type SMTPTransport from 'nodemailer/lib/smtp-transport/index.js';
import { SocksClient } from 'socks';
import { resolveAccountSecret } from '../oauth.js';
import type { AccountSecret, MailAccount } from '../types.js';

export function address(value?: { name?: string; address?: string } | null) {
  return { name: value?.name ?? '', address: value?.address ?? '' };
}

function authFor(account: MailAccount, secret: AccountSecret) {
  return secret.accessToken
    ? { user: account.email, accessToken: secret.accessToken }
    : { user: account.email, pass: secret.password ?? '' };
}

export function proxyUrlFor(account: MailAccount, secret: AccountSecret) {
  if (!account.proxy) return undefined;
  const host = account.proxy.host.includes(':') ? `[${account.proxy.host}]` : account.proxy.host;
  const auth = account.proxy.username
    ? `${encodeURIComponent(account.proxy.username)}:${encodeURIComponent(secret.proxyPassword ?? '')}@`
    : '';
  return `${account.proxy.protocol}://${auth}${host}:${account.proxy.port}`;
}

export type ImapClientOptions = {
  disableAutoIdle?: boolean;
  maxIdleTime?: number;
  missingIdleCommand?: 'NOOP' | 'SELECT' | 'STATUS';
};

export async function imapClientFor(account: MailAccount, options: ImapClientOptions = {}) {
  const secret = await resolveAccountSecret(account);
  return new ImapFlow({
    host: account.settings.imapHost,
    port: account.settings.imapPort,
    secure: account.settings.imapSecure,
    auth: authFor(account, secret),
    proxy: proxyUrlFor(account, secret),
    logger: false,
    qresync: true,
    disableAutoIdle: options.disableAutoIdle,
    maxIdleTime: options.maxIdleTime ?? 4 * 60_000,
    missingIdleCommand: options.missingIdleCommand ?? 'NOOP',
    connectionTimeout: 30_000,
    greetingTimeout: 30_000,
    socketTimeout: 120_000,
  });
}

export function smtpTransport(account: MailAccount, secret: AccountSecret) {
  const auth = secret.accessToken
    ? { type: 'OAuth2' as const, user: account.email, accessToken: secret.accessToken }
    : { user: account.email, pass: secret.password ?? '' };
  const yahooOAuthBearer = secret.oauthProvider === 'yahoo' && secret.accessToken;
  const transportOptions: SMTPTransport.Options & { proxy?: string } = {
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
          '', '',
        ].join('\x01');
        const response = await context.sendCommand(`AUTH OAUTHBEARER ${Buffer.from(payload).toString('base64')}`);
        if (response.status !== 235) throw new Error(`Yahoo SMTP OAuth 验证失败 (${response.status})`);
        return true;
      },
    } : undefined,
    disableFileAccess: true,
    disableUrlAccess: true,
    proxy: proxyUrlFor(account, secret),
  };
  const transport = nodemailer.createTransport(transportOptions);
  if (account.proxy?.protocol === 'socks5') transport.set('proxy_socks_module', { SocksClient });
  return transport;
}

type ProtocolError = Error & {
  responseText?: unknown; response?: unknown; responseStatus?: unknown; serverResponseCode?: unknown; code?: unknown;
};

export function describeProtocolError(stage: 'IMAP' | 'SMTP', error: unknown): Error {
  const value = error instanceof Error ? error as ProtocolError : undefined;
  const responseText = typeof value?.responseText === 'string' ? value.responseText : undefined;
  const response = typeof value?.response === 'string' ? value.response : undefined;
  const generic = value?.message === 'Command failed' ? undefined : value?.message;
  const detail = responseText || response || generic || '服务商拒绝了连接请求';
  const status = [value?.serverResponseCode, value?.responseStatus, value?.code]
    .find((item) => typeof item === 'string' || typeof item === 'number');
  const safeDetail = String(detail).replace(/[\r\n]+/g, ' ').replace(/\s+/g, ' ')
    .replace(/Bearer\s+[^\s,;]+/gi, 'Bearer [redacted]')
    .replace(/(access[_-]?token|refresh[_-]?token|password|authorization)(\s*[:=]\s*)[^\s,;]+/gi, '$1$2[redacted]')
    .trim().slice(0, 500);
  return new Error(`${stage} 验证失败${status ? ` (${String(status)})` : ''}：${safeDetail}`);
}

export async function testAccount(account: MailAccount): Promise<void> {
  const secret = await resolveAccountSecret(account);
  const client = new ImapFlow({
    host: account.settings.imapHost, port: account.settings.imapPort, secure: account.settings.imapSecure,
    auth: authFor(account, secret), logger: false,
    proxy: proxyUrlFor(account, secret),
    qresync: true, maxIdleTime: 4 * 60_000, missingIdleCommand: 'NOOP', connectionTimeout: 30_000, greetingTimeout: 30_000, socketTimeout: 120_000,
  });
  try {
    try { await client.connect(); await client.mailboxOpen('INBOX', { readOnly: true }); }
    catch (error) { throw describeProtocolError('IMAP', error); }
  } finally { await client.logout().catch(() => undefined); }
  const transport = smtpTransport(account, secret);
  try {
    try { await transport.verify(); }
    catch (error) { throw describeProtocolError('SMTP', error); }
  } finally { transport.close(); }
}
