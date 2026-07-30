import crypto from 'node:crypto';
import { decryptPayload, encryptPayload, encryptSecret } from '../crypto.js';
import { settingsFor } from '../providers.js';
import { readStore, updateStore } from '../store.js';
import type { MailAccount, ProviderId } from '../types.js';
import { fetchIdentity, tokenRequest, tokenToSecret } from './client.js';
import { describeOAuthCallbackError, oauthKeyFor, providerConfig, type OAuthProviderKey } from './config.js';
import { validateStoredAccountConnection } from './secrets.js';
import { currentUserId, enterUserContext } from '../auth/context.js';

type PendingOAuth = {
  ownerId: string;
  providerKey: OAuthProviderKey;
  accountProvider: ProviderId;
  codeVerifier: string;
  nonce: string;
  createdAt: number;
  displayName?: string;
  group: string;
  color: string;
  accountId?: string;
  expectedEmail?: string;
};

const completed = new Map<string, { accountId: string; ownerId: string; completedAt: number }>();

function cleanupCompleted() {
  const cutoff = Date.now() - 10 * 60_000;
  for (const [state, value] of completed) if (value.completedAt < cutoff) completed.delete(state);
}

export async function beginOAuth(input: { provider: ProviderId; displayName?: string; group?: string; color?: string; accountId?: string; expectedEmail?: string }) {
  cleanupCompleted();
  const providerKey = oauthKeyFor(input.provider);
  if (!providerKey) throw new Error(`${input.provider} 没有公开可用的邮件 OAuth 接口，请使用应用专用密码或授权码`);
  const config = providerConfig(providerKey, input.provider);
  if (!config.configured) throw new Error(config.configurationHint);
  const nonce = crypto.randomBytes(24).toString('base64url');
  const codeVerifier = crypto.randomBytes(48).toString('base64url');
  const codeChallenge = crypto.createHash('sha256').update(codeVerifier).digest('base64url');
  const ownerId = currentUserId() ?? '__legacy__';
  const session: PendingOAuth = {
    ownerId,
    providerKey, accountProvider: input.provider, codeVerifier, nonce, createdAt: Date.now(), displayName: input.displayName,
    group: input.group?.trim() || '个人', color: input.color || '#168f78', accountId: input.accountId, expectedEmail: input.expectedEmail?.toLowerCase(),
  };
  const state = await encryptPayload(session);
  const url = new URL(config.authorizationEndpoint);
  url.searchParams.set('client_id', config.clientId!);
  url.searchParams.set('redirect_uri', config.redirectUri);
  url.searchParams.set('response_type', 'code');
  url.searchParams.set('scope', config.scopes.join(' '));
  url.searchParams.set('state', state);
  url.searchParams.set('nonce', nonce);
  url.searchParams.set('code_challenge', codeChallenge);
  url.searchParams.set('code_challenge_method', 'S256');
  if (providerKey === 'google') {
    url.searchParams.set('access_type', 'offline'); url.searchParams.set('prompt', 'consent select_account'); url.searchParams.set('include_granted_scopes', 'true');
  } else if (providerKey === 'microsoft') url.searchParams.set('prompt', 'select_account');
  return { authorizationUrl: url.toString(), state, provider: providerKey };
}

export async function beginOAuthReconnect(account: MailAccount) {
  if (account.authMethod !== 'oauth2') throw new Error('这个邮箱不是通过 OAuth 接入的');
  return beginOAuth({ provider: account.provider, displayName: account.displayName, group: account.group, color: account.color, accountId: account.id, expectedEmail: account.email });
}

export async function completeOAuth(input: { providerKey: OAuthProviderKey; state?: string; code?: string; error?: string; errorDescription?: string }) {
  cleanupCompleted();
  const remembered = input.state ? completed.get(input.state) : undefined;
  if (remembered) {
    enterUserContext(remembered.ownerId);
    const existing = await readStore();
    const account = existing.accounts.find((item) => item.id === remembered.accountId);
    if (account) return account;
    completed.delete(input.state!);
  }
  if (input.error) throw new Error(describeOAuthCallbackError(input.error, input.errorDescription));
  if (!input.state || !input.code) throw new Error('OAuth 回调缺少 code 或 state');
  let session: PendingOAuth;
  try { session = await decryptPayload<PendingOAuth>(input.state); }
  catch { throw new Error('OAuth state 无效或已过期，请重新开始'); }
  if (session.providerKey !== input.providerKey || Date.now() - session.createdAt > 10 * 60_000) throw new Error('OAuth state 无效或已过期，请重新开始');
  if (!session.ownerId) throw new Error('OAuth state 缺少应用账号归属，请重新开始');
  enterUserContext(session.ownerId);
  const config = providerConfig(input.providerKey, session.accountProvider);
  const token = await tokenRequest(config, new URLSearchParams({ grant_type: 'authorization_code', code: input.code, redirect_uri: config.redirectUri, code_verifier: session.codeVerifier }));
  const identity = await fetchIdentity(config, token, session.nonce);
  const existing = await readStore();
  const secret = tokenToSecret(config, token);
  if (session.accountId) {
    const current = existing.accounts.find((account) => account.id === session.accountId);
    if (!current) throw new Error('需要重新授权的邮箱已不存在');
    if (current.provider !== session.accountProvider) throw new Error('邮箱服务商与重新授权请求不匹配');
    if (identity.email !== session.expectedEmail || identity.email !== current.email.toLowerCase()) throw new Error(`请使用原邮箱 ${current.email} 登录，不能切换为 ${identity.email}`);
    const reconnected: MailAccount = { ...current, encryptedSecret: await encryptSecret(secret), authMethod: 'oauth2', status: 'syncing', lastError: undefined };
    await updateStore((data) => {
      const index = data.accounts.findIndex((account) => account.id === reconnected.id);
      if (index < 0) throw new Error('需要重新授权的邮箱已不存在');
      data.accounts[index] = reconnected;
    });
    const validated = await validateStoredAccountConnection(reconnected);
    completed.set(input.state, { accountId: validated.id, ownerId: session.ownerId, completedAt: Date.now() });
    return validated;
  }
  if (existing.accounts.some((account) => account.email === identity.email)) throw new Error('这个邮箱已经添加');
  const account: MailAccount = {
    id: crypto.randomUUID(), provider: session.accountProvider, email: identity.email,
    displayName: session.displayName?.trim() || identity.name || identity.email, group: session.group, color: session.color,
    settings: settingsFor(session.accountProvider), encryptedSecret: await encryptSecret(secret), authMethod: 'oauth2',
    createdAt: new Date().toISOString(), status: 'syncing',
  };
  await updateStore((data) => {
    if (data.accounts.some((item) => item.email === account.email)) throw new Error('这个邮箱刚刚被其他操作添加');
    data.accounts.push(account);
  });
  const validated = await validateStoredAccountConnection(account);
  completed.set(input.state, { accountId: validated.id, ownerId: session.ownerId, completedAt: Date.now() });
  return validated;
}
