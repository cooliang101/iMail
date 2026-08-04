import { createCipheriv, randomBytes, scrypt } from 'node:crypto';
import { currentUserId } from '../auth/context.js';
import { decryptSecret } from '../crypto.js';
import { readStore, updateStore } from '../store.js';
import type { AccountSecret, MailAccount } from '../types.js';

export const MAIL_AUTHORIZATION_EXPORT_FORMAT = 'imail-mail-authorizations';
export const MAIL_AUTHORIZATION_EXPORT_VERSION = 1;
export const MAIL_AUTHORIZATION_EXPORT_TTL_MS = 2 * 60_000;

const SCRYPT_COST = 32_768;
const SCRYPT_BLOCK_SIZE = 8;
const SCRYPT_PARALLELIZATION = 1;
const SCRYPT_KEY_LENGTH = 32;
const SCRYPT_MAX_MEMORY = 64 * 1024 * 1024;
const MAX_PENDING_EXPORTS = 100;
const privacyActionQueues = new Map<string, Promise<void>>();

type ExplicitAccountSecret = {
  authType?: AccountSecret['authType'];
  password?: string;
  accessToken?: string;
  refreshToken?: string;
  expiresAt?: string;
  oauthProvider?: AccountSecret['oauthProvider'];
  scopes?: string[];
  tokenType?: string;
  proxyPassword?: string;
};

export type MailAuthorizationExportPayload = {
  format: typeof MAIL_AUTHORIZATION_EXPORT_FORMAT;
  formatVersion: typeof MAIL_AUTHORIZATION_EXPORT_VERSION;
  exportedAt: string;
  accounts: Array<{
    provider: MailAccount['provider'];
    email: string;
    displayName: string;
    group: string;
    groupIcon: NonNullable<MailAccount['groupIcon']>;
    color: string;
    authMethod: NonNullable<MailAccount['authMethod']>;
    settings: {
      imapHost: string;
      imapPort: number;
      imapSecure: boolean;
      smtpHost: string;
      smtpPort: number;
      smtpSecure: boolean;
    };
    proxy?: {
      protocol: NonNullable<MailAccount['proxy']>['protocol'];
      host: string;
      port: number;
      username?: string;
    };
    authorization: ExplicitAccountSecret;
  }>;
};

export type MailAuthorizationExportEnvelope = {
  format: typeof MAIL_AUTHORIZATION_EXPORT_FORMAT;
  formatVersion: typeof MAIL_AUTHORIZATION_EXPORT_VERSION;
  kdf: {
    algorithm: 'scrypt';
    salt: string;
    cost: number;
    blockSize: number;
    parallelization: number;
    keyLength: number;
  };
  cipher: {
    algorithm: 'aes-256-gcm';
    iv: string;
    authTag: string;
  };
  ciphertext: string;
};

type PendingExport = {
  id: string;
  userId: string;
  body: Buffer;
  filename: string;
  accountCount: number;
  expiresAt: string;
  timer: NodeJS.Timeout;
};

const pendingExports = new Map<string, PendingExport>();

async function withUserPrivacyLock<T>(userId: string, action: () => Promise<T>): Promise<T> {
  const previous = privacyActionQueues.get(userId) ?? Promise.resolve();
  const operation = previous.catch(() => undefined).then(action);
  const tail = operation.then(() => undefined, () => undefined);
  privacyActionQueues.set(userId, tail);
  try {
    return await operation;
  } finally {
    if (privacyActionQueues.get(userId) === tail) privacyActionQueues.delete(userId);
  }
}

function explicitSecret(secret: AccountSecret): ExplicitAccountSecret {
  const output: ExplicitAccountSecret = {};
  if (secret.authType !== undefined) output.authType = secret.authType;
  if (secret.password !== undefined) output.password = secret.password;
  if (secret.accessToken !== undefined) output.accessToken = secret.accessToken;
  if (secret.refreshToken !== undefined) output.refreshToken = secret.refreshToken;
  if (secret.expiresAt !== undefined) output.expiresAt = secret.expiresAt;
  if (secret.oauthProvider !== undefined) output.oauthProvider = secret.oauthProvider;
  if (secret.scopes !== undefined) output.scopes = [...secret.scopes];
  if (secret.tokenType !== undefined) output.tokenType = secret.tokenType;
  if (secret.proxyPassword !== undefined) output.proxyPassword = secret.proxyPassword;
  return output;
}

async function buildPayload(accounts: MailAccount[]): Promise<MailAuthorizationExportPayload> {
  return {
    format: MAIL_AUTHORIZATION_EXPORT_FORMAT,
    formatVersion: MAIL_AUTHORIZATION_EXPORT_VERSION,
    exportedAt: new Date().toISOString(),
    accounts: await Promise.all(accounts.map(async (account) => {
      const authorization = explicitSecret(await decryptSecret(account.encryptedSecret));
      return {
        provider: account.provider,
        email: account.email,
        displayName: account.displayName,
        group: account.group,
        groupIcon: account.groupIcon ?? 'folder',
        color: account.color,
        authMethod: account.authMethod ?? authorization.authType ?? (authorization.accessToken || authorization.refreshToken ? 'oauth2' : 'app-password'),
        settings: {
          imapHost: account.settings.imapHost,
          imapPort: account.settings.imapPort,
          imapSecure: account.settings.imapSecure,
          smtpHost: account.settings.smtpHost,
          smtpPort: account.settings.smtpPort,
          smtpSecure: account.settings.smtpSecure,
        },
        ...(account.proxy ? { proxy: {
          protocol: account.proxy.protocol,
          host: account.proxy.host,
          port: account.proxy.port,
          ...(account.proxy.username !== undefined ? { username: account.proxy.username } : {}),
        } } : {}),
        authorization,
      };
    })),
  };
}

function deriveExportKey(password: string, salt: Buffer) {
  return new Promise<Buffer>((resolve, reject) => {
    scrypt(password, salt, SCRYPT_KEY_LENGTH, {
      N: SCRYPT_COST,
      r: SCRYPT_BLOCK_SIZE,
      p: SCRYPT_PARALLELIZATION,
      maxmem: SCRYPT_MAX_MEMORY,
    }, (error, key) => error ? reject(error) : resolve(key));
  });
}

async function encryptPayload(payload: MailAuthorizationExportPayload, password: string) {
  const salt = randomBytes(16);
  const iv = randomBytes(12);
  const key = await deriveExportKey(password, salt);
  const plaintext = Buffer.from(JSON.stringify(payload), 'utf8');
  const authenticatedData = Buffer.from(`${MAIL_AUTHORIZATION_EXPORT_FORMAT}:v${MAIL_AUTHORIZATION_EXPORT_VERSION}`, 'utf8');
  try {
    const cipher = createCipheriv('aes-256-gcm', key, iv);
    cipher.setAAD(authenticatedData);
    const ciphertext = Buffer.concat([cipher.update(plaintext), cipher.final()]);
    return {
      format: MAIL_AUTHORIZATION_EXPORT_FORMAT,
      formatVersion: MAIL_AUTHORIZATION_EXPORT_VERSION,
      kdf: {
        algorithm: 'scrypt',
        salt: salt.toString('base64url'),
        cost: SCRYPT_COST,
        blockSize: SCRYPT_BLOCK_SIZE,
        parallelization: SCRYPT_PARALLELIZATION,
        keyLength: SCRYPT_KEY_LENGTH,
      },
      cipher: {
        algorithm: 'aes-256-gcm',
        iv: iv.toString('base64url'),
        authTag: cipher.getAuthTag().toString('base64url'),
      },
      ciphertext: ciphertext.toString('base64url'),
    } satisfies MailAuthorizationExportEnvelope;
  } finally {
    key.fill(0);
    plaintext.fill(0);
  }
}

function disposePendingExport(id: string) {
  const pending = pendingExports.get(id);
  if (!pending) return;
  pendingExports.delete(id);
  clearTimeout(pending.timer);
  pending.body.fill(0);
}

function enforcePendingExportLimit() {
  while (pendingExports.size >= MAX_PENDING_EXPORTS) {
    const oldest = pendingExports.keys().next().value as string | undefined;
    if (!oldest) break;
    disposePendingExport(oldest);
  }
}

export async function prepareMailAuthorizationExport(userId: string, exportPassword: string) {
  if (currentUserId() !== userId) throw new Error('邮箱授权导出缺少匹配的用户上下文');
  return withUserPrivacyLock(userId, async () => {
    for (const [id, pending] of pendingExports) if (pending.userId === userId) disposePendingExport(id);
    enforcePendingExportLimit();

    const accounts = (await readStore()).accounts;
    const payload = await buildPayload(accounts);
    const envelope = await encryptPayload(payload, exportPassword);
    const body = Buffer.from(`${JSON.stringify(envelope, null, 2)}\n`, 'utf8');
    const id = randomBytes(32).toString('base64url');
    const expiresAt = new Date(Date.now() + MAIL_AUTHORIZATION_EXPORT_TTL_MS).toISOString();
    const filename = `imail-mail-authorizations-${new Date().toISOString().slice(0, 10)}.imailauth`;
    const timer = setTimeout(() => disposePendingExport(id), MAIL_AUTHORIZATION_EXPORT_TTL_MS);
    timer.unref();
    pendingExports.set(id, { id, userId, body, filename, accountCount: accounts.length, expiresAt, timer });
    return { id, filename, accountCount: accounts.length, expiresAt };
  });
}

export function consumeMailAuthorizationExport(id: string, userId: string, now = Date.now()) {
  const pending = pendingExports.get(id);
  if (!pending || pending.userId !== userId || new Date(pending.expiresAt).getTime() <= now) {
    if (pending && new Date(pending.expiresAt).getTime() <= now) disposePendingExport(id);
    return undefined;
  }
  pendingExports.delete(id);
  clearTimeout(pending.timer);
  return pending;
}

export function discardUserMailAuthorizationExports(userId: string) {
  for (const [id, pending] of pendingExports) if (pending.userId === userId) disposePendingExport(id);
}

export async function clearCurrentUserMailData(userId: string) {
  if (currentUserId() !== userId) throw new Error('邮箱数据清除缺少匹配的用户上下文');
  return withUserPrivacyLock(userId, async () => {
    let accountCount = 0;
    await updateStore((data) => {
      accountCount = data.accounts.length;
      data.accounts = [];
      data.messages = [];
      data.tokens = [];
      data.drafts = [];
      data.contacts = [];
      data.logoFetchAttempts = [];
    });
    discardUserMailAuthorizationExports(userId);
    return { accountCount };
  });
}

export const privacyInternals = {
  buildPayload,
  encryptPayload,
  pendingExportCount: () => pendingExports.size,
  disposePendingExport,
};
