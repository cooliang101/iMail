import crypto from 'node:crypto';
import { encryptSecret } from '../crypto.js';
import { testAccount } from '../mail.js';
import { settingsFor } from '../providers.js';
import { readStore, updateStore } from '../store.js';
import { getSyncStore } from '../sync/store.js';
import type { MailAccount, MailSettings, ProviderId, WorkspaceIconId } from '../types.js';
import { conflict, invalid, notFound } from './errors.js';

export type CreateAccountInput = {
  provider: ProviderId;
  email: string;
  displayName: string;
  group: string;
  groupIcon?: WorkspaceIconId;
  color: string;
  password?: string;
  accessToken?: string;
  settings?: MailSettings;
};

export async function accountById(id: string) {
  const account = (await readStore()).accounts.find((item) => item.id === id);
  if (!account) throw notFound('ACCOUNT_NOT_FOUND', '邮箱账户不存在');
  return account;
}

export async function accountByEmail(email: string) {
  const normalized = email.toLowerCase();
  const account = (await readStore()).accounts.find((item) => item.email.toLowerCase() === normalized);
  if (!account) throw notFound('ACCOUNT_NOT_FOUND', `邮箱账户不存在：${email}`);
  return account;
}

export async function createAccount(input: CreateAccountInput) {
  const email = input.email.toLowerCase();
  if ((await readStore()).accounts.some((item) => item.email === email)) throw conflict('ACCOUNT_EXISTS', '这个邮箱已经添加');
  if (!input.password && !input.accessToken) throw invalid('ACCOUNT_CREDENTIAL_REQUIRED', '请填写应用专用密码或 OAuth Access Token');
  const secret = input.password
    ? { authType: 'app-password' as const, password: input.password }
    : { authType: 'oauth2' as const, accessToken: input.accessToken };
  const account: MailAccount = {
    id: crypto.randomUUID(), provider: input.provider, email, displayName: input.displayName.trim(), group: input.group.trim(),
    groupIcon: input.groupIcon, color: input.color, settings: settingsFor(input.provider, input.settings),
    encryptedSecret: await encryptSecret(secret),
    authMethod: input.password ? 'app-password' : 'oauth2', createdAt: new Date().toISOString(), status: 'connected',
  };
  await testAccount(account);
  await updateStore((data) => {
    if (data.accounts.some((item) => item.email === email)) throw conflict('ACCOUNT_EXISTS', '这个邮箱刚刚被其他操作添加');
    data.accounts.push(account);
  });
  getSyncStore().ensurePolicy(account.id);
  return account;
}

export async function updateAccountMetadata(id: string, changes: Partial<Pick<MailAccount, 'displayName' | 'group' | 'groupIcon' | 'color'>>) {
  let updated: MailAccount | undefined;
  await updateStore((data) => {
    updated = data.accounts.find((item) => item.id === id);
    if (!updated) throw notFound('ACCOUNT_NOT_FOUND', '邮箱账户不存在');
    Object.assign(updated, changes);
  });
  return updated!;
}

export async function replaceAccountPassword(id: string, password: string) {
  const account = await accountById(id);
  if (account.authMethod === 'oauth2') throw conflict('OAUTH_RECONNECT_REQUIRED', 'OAuth 邮箱请使用重新授权');
  const candidate: MailAccount = {
    ...account, encryptedSecret: await encryptSecret({ authType: 'app-password', password }),
    authMethod: 'app-password', status: 'syncing', lastError: undefined,
  };
  await testAccount(candidate);
  const connected: MailAccount = { ...candidate, status: 'connected' };
  await updateStore((data) => {
    const index = data.accounts.findIndex((item) => item.id === id);
    if (index < 0) throw notFound('ACCOUNT_NOT_FOUND', '邮箱账户已被移除');
    data.accounts[index] = connected;
  });
  return connected;
}

export async function removeAccount(id: string) {
  const account = await accountById(id);
  await updateStore((data) => {
    if (!data.accounts.some((item) => item.id === id)) throw notFound('ACCOUNT_NOT_FOUND', '邮箱账户已被移除');
    data.accounts = data.accounts.filter((item) => item.id !== id);
    data.messages = data.messages.filter((item) => item.accountId !== id);
    data.drafts = (data.drafts ?? []).filter((item) => item.accountId !== id);
    data.tokens.forEach((token) => { token.accountIds = token.accountIds.filter((accountId) => accountId !== id); });
  });
  return account;
}
