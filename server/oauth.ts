import crypto from 'node:crypto';
import { createRemoteJWKSet, jwtVerify, type JWTPayload } from 'jose';
import { decryptPayload, decryptSecret, encryptPayload, encryptSecret } from './crypto.js';
import { settingsFor } from './providers.js';
import { readStore, updateStore } from './store.js';
import type { AccountSecret, MailAccount, ProviderId } from './types.js';

export type OAuthProviderKey = 'google' | 'microsoft' | 'yahoo';

type OAuthConfig = {
  key: OAuthProviderKey;
  clientId?: string;
  clientSecret?: string;
  redirectUri: string;
  authorizationEndpoint: string;
  tokenEndpoint: string;
  userInfoEndpoint?: string;
  jwksUri?: string;
  scopes: string[];
  configured: boolean;
  configurationHint: string;
};

type PendingOAuth = {
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

type OAuthTokenResponse = {
  access_token: string;
  refresh_token?: string;
  expires_in?: number;
  token_type?: string;
  scope?: string;
  id_token?: string;
  error?: string;
  error_description?: string;
};

const completed = new Map<string, { accountId: string; completedAt: number }>();
const refreshes = new Map<string, Promise<AccountSecret>>();
const callbackBase = process.env.OAUTH_CALLBACK_BASE_URL ?? `http://localhost:${process.env.PORT ?? 8787}/api/oauth`;

function providerConfig(key: OAuthProviderKey, accountProvider?: ProviderId): OAuthConfig {
  if (key === 'google') {
    const clientId = process.env.GOOGLE_OAUTH_CLIENT_ID;
    const clientSecret = process.env.GOOGLE_OAUTH_CLIENT_SECRET;
    return {
      key, clientId, clientSecret,
      redirectUri: process.env.GOOGLE_OAUTH_REDIRECT_URI ?? `${callbackBase}/google/callback`,
      authorizationEndpoint: 'https://accounts.google.com/o/oauth2/v2/auth',
      tokenEndpoint: 'https://oauth2.googleapis.com/token',
      userInfoEndpoint: 'https://openidconnect.googleapis.com/v1/userinfo',
      scopes: ['openid', 'email', 'profile', 'https://mail.google.com/'],
      configured: Boolean(clientId && clientSecret),
      configurationHint: '配置 GOOGLE_OAUTH_CLIENT_ID 与 GOOGLE_OAUTH_CLIENT_SECRET，并在 Google Cloud Console 登记回调地址。',
    };
  }
  if (key === 'microsoft') {
    const clientId = process.env.MICROSOFT_OAUTH_CLIENT_ID;
    const tenant = accountProvider === 'hotmail' ? 'consumers' : 'common';
    return {
      key, clientId, clientSecret: process.env.MICROSOFT_OAUTH_CLIENT_SECRET,
      redirectUri: process.env.MICROSOFT_OAUTH_REDIRECT_URI ?? `${callbackBase}/microsoft/callback`,
      authorizationEndpoint: `https://login.microsoftonline.com/${tenant}/oauth2/v2.0/authorize`,
      tokenEndpoint: `https://login.microsoftonline.com/${tenant}/oauth2/v2.0/token`,
      jwksUri: 'https://login.microsoftonline.com/common/discovery/v2.0/keys',
      scopes: ['openid', 'email', 'profile', 'offline_access', 'https://outlook.office.com/IMAP.AccessAsUser.All', 'https://outlook.office.com/SMTP.Send'],
      configured: Boolean(clientId),
      configurationHint: '配置 MICROSOFT_OAUTH_CLIENT_ID；机密 Web 应用还应配置 MICROSOFT_OAUTH_CLIENT_SECRET，并在 Entra 登记回调地址。',
    };
  }
  const clientId = process.env.YAHOO_OAUTH_CLIENT_ID;
  const clientSecret = process.env.YAHOO_OAUTH_CLIENT_SECRET;
  const approved = process.env.YAHOO_MAIL_OAUTH_APPROVED === 'true';
  return {
    key, clientId, clientSecret,
    redirectUri: process.env.YAHOO_OAUTH_REDIRECT_URI ?? `${callbackBase}/yahoo/callback`,
    authorizationEndpoint: 'https://api.login.yahoo.com/oauth2/request_auth',
    tokenEndpoint: 'https://api.login.yahoo.com/oauth2/get_token',
    userInfoEndpoint: 'https://api.login.yahoo.com/openid/v1/userinfo',
    scopes: ['openid', 'email', 'profile', 'mail-r', 'mail-w'],
    configured: Boolean(clientId && clientSecret && approved),
    configurationHint: 'Yahoo mail-r/mail-w 是受限权限。审核通过后配置 YAHOO_OAUTH_CLIENT_ID、YAHOO_OAUTH_CLIENT_SECRET 与 YAHOO_MAIL_OAUTH_APPROVED=true。',
  };
}

function oauthKeyFor(provider: ProviderId): OAuthProviderKey | null {
  if (provider === 'gmail') return 'google';
  if (provider === 'outlook' || provider === 'hotmail') return 'microsoft';
  if (provider === 'yahoo') return 'yahoo';
  return null;
}

export function oauthProviderCatalog() {
  return (['google', 'microsoft', 'yahoo'] as const).map((key) => {
    const config = providerConfig(key);
    return {
      id: key,
      configured: config.configured,
      redirectUri: config.redirectUri,
      scopes: config.scopes,
      configurationHint: config.configurationHint,
    };
  });
}

export function describeOAuthCallbackError(error: string, description?: string) {
  const detail = description?.trim();
  if (detail) return detail;
  if (error === 'access_denied') return '你已取消或拒绝授权，邮箱没有发生更改';
  if (error === 'server_error' || error === 'temporarily_unavailable') return '服务商暂时未能完成授权。iMail 会先检查授权是否已经保存；若账户仍未出现，请稍后重试';
  if (error === 'invalid_request' || error === 'unauthorized_client') return 'OAuth 应用或回调地址配置不正确，请检查服务商开发者控制台';
  return `授权未完成：${error}`;
}

function base64Url(bytes: Buffer) {
  return bytes.toString('base64url');
}

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
  const nonce = base64Url(crypto.randomBytes(24));
  const codeVerifier = base64Url(crypto.randomBytes(48));
  const codeChallenge = crypto.createHash('sha256').update(codeVerifier).digest('base64url');
  const session: PendingOAuth = {
    providerKey, accountProvider: input.provider, codeVerifier, nonce,
    createdAt: Date.now(), displayName: input.displayName,
    group: input.group?.trim() || '个人', color: input.color || '#168f78',
    accountId: input.accountId, expectedEmail: input.expectedEmail?.toLowerCase(),
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
    url.searchParams.set('access_type', 'offline');
    url.searchParams.set('prompt', 'consent select_account');
    url.searchParams.set('include_granted_scopes', 'true');
  } else if (providerKey === 'microsoft') {
    url.searchParams.set('prompt', 'select_account');
  }
  return { authorizationUrl: url.toString(), state, provider: providerKey };
}

export async function beginOAuthReconnect(account: MailAccount) {
  if (account.authMethod !== 'oauth2') throw new Error('这个邮箱不是通过 OAuth 接入的');
  return beginOAuth({
    provider: account.provider,
    displayName: account.displayName,
    group: account.group,
    color: account.color,
    accountId: account.id,
    expectedEmail: account.email,
  });
}

async function tokenRequest(config: OAuthConfig, parameters: URLSearchParams): Promise<OAuthTokenResponse> {
  parameters.set('client_id', config.clientId!);
  if (config.clientSecret && config.key !== 'yahoo') parameters.set('client_secret', config.clientSecret);
  const headers: Record<string, string> = { 'Content-Type': 'application/x-www-form-urlencoded', Accept: 'application/json' };
  if (config.key === 'yahoo' && config.clientSecret) {
    headers.Authorization = `Basic ${Buffer.from(`${config.clientId}:${config.clientSecret}`).toString('base64')}`;
  }
  const response = await fetch(config.tokenEndpoint, { method: 'POST', headers, body: parameters });
  const body = await response.json() as OAuthTokenResponse;
  if (!response.ok || body.error || !body.access_token) throw new Error(body.error_description || body.error || `OAuth Token 交换失败 (${response.status})`);
  return body;
}

async function fetchIdentity(config: OAuthConfig, token: OAuthTokenResponse, nonce: string): Promise<{ email: string; name?: string }> {
  if (config.key === 'microsoft') {
    if (!token.id_token || !config.jwksUri) throw new Error('Microsoft 未返回 ID Token');
    const result = await jwtVerify(token.id_token, createRemoteJWKSet(new URL(config.jwksUri)), { audience: config.clientId });
    const payload = result.payload as JWTPayload & { preferred_username?: string; email?: string; name?: string; nonce?: string; tid?: string };
    if (payload.nonce !== nonce) throw new Error('OAuth nonce 校验失败');
    if (!payload.iss?.startsWith('https://login.microsoftonline.com/') || !payload.iss.endsWith('/v2.0')) throw new Error('Microsoft Token 签发方无效');
    const email = payload.preferred_username || payload.email;
    if (!email) throw new Error('Microsoft 账户没有可用邮箱地址');
    return { email: email.toLowerCase(), name: payload.name };
  }
  if (!config.userInfoEndpoint) throw new Error('OAuth 身份端点未配置');
  const response = await fetch(config.userInfoEndpoint, { headers: { Authorization: `Bearer ${token.access_token}`, Accept: 'application/json' } });
  const profile = await response.json() as { email?: string; name?: string };
  if (!response.ok || !profile.email) throw new Error('OAuth 登录成功，但未能读取邮箱地址');
  return { email: profile.email.toLowerCase(), name: profile.name };
}

function tokenToSecret(config: OAuthConfig, token: OAuthTokenResponse, previousRefreshToken?: string): AccountSecret {
  return {
    authType: 'oauth2', oauthProvider: config.key,
    accessToken: token.access_token,
    refreshToken: token.refresh_token || previousRefreshToken,
    expiresAt: new Date(Date.now() + Math.max(60, token.expires_in ?? 3600) * 1000).toISOString(),
    scopes: (token.scope || config.scopes.join(' ')).split(/\s+/).filter(Boolean),
    tokenType: token.token_type || 'Bearer',
  };
}

export async function completeOAuth(input: { providerKey: OAuthProviderKey; state?: string; code?: string; error?: string; errorDescription?: string }) {
  cleanupCompleted();
  const remembered = input.state ? completed.get(input.state) : undefined;
  if (remembered) {
    const existing = await readStore();
    const account = existing.accounts.find((item) => item.id === remembered.accountId);
    if (account) return account;
    completed.delete(input.state!);
  }
  if (input.error) throw new Error(describeOAuthCallbackError(input.error, input.errorDescription));
  if (!input.state || !input.code) throw new Error('OAuth 回调缺少 code 或 state');
  let session: PendingOAuth;
  try {
    session = await decryptPayload<PendingOAuth>(input.state);
  } catch {
    throw new Error('OAuth state 无效或已过期，请重新开始');
  }
  if (session.providerKey !== input.providerKey || Date.now() - session.createdAt > 10 * 60_000) throw new Error('OAuth state 无效或已过期，请重新开始');
  const config = providerConfig(input.providerKey, session.accountProvider);
  const token = await tokenRequest(config, new URLSearchParams({
    grant_type: 'authorization_code', code: input.code,
    redirect_uri: config.redirectUri, code_verifier: session.codeVerifier,
  }));
  const identity = await fetchIdentity(config, token, session.nonce);
  const existing = await readStore();
  const secret = tokenToSecret(config, token);
  if (session.accountId) {
    const current = existing.accounts.find((account) => account.id === session.accountId);
    if (!current) throw new Error('需要重新授权的邮箱已不存在');
    if (current.provider !== session.accountProvider) throw new Error('邮箱服务商与重新授权请求不匹配');
    if (identity.email !== session.expectedEmail || identity.email !== current.email.toLowerCase()) {
      throw new Error(`请使用原邮箱 ${current.email} 登录，不能切换为 ${identity.email}`);
    }
    const reconnected: MailAccount = {
      ...current,
      encryptedSecret: await encryptSecret(secret),
      authMethod: 'oauth2',
      status: 'syncing',
      lastError: undefined,
    };
    await updateStore((data) => {
      const index = data.accounts.findIndex((account) => account.id === reconnected.id);
      if (index < 0) throw new Error('需要重新授权的邮箱已不存在');
      data.accounts[index] = reconnected;
    });
    const validated = await validateStoredAccountConnection(reconnected);
    completed.set(input.state, { accountId: validated.id, completedAt: Date.now() });
    return validated;
  }
  if (existing.accounts.some((account) => account.email === identity.email)) throw new Error('这个邮箱已经添加');
  const account: MailAccount = {
    id: crypto.randomUUID(), provider: session.accountProvider,
    email: identity.email,
    displayName: session.displayName?.trim() || identity.name || identity.email,
    group: session.group, color: session.color,
    settings: settingsFor(session.accountProvider),
    encryptedSecret: await encryptSecret(secret), authMethod: 'oauth2',
    createdAt: new Date().toISOString(), status: 'syncing',
  };
  await updateStore((data) => {
    if (data.accounts.some((item) => item.email === account.email)) throw new Error('这个邮箱刚刚被其他操作添加');
    data.accounts.push(account);
  });
  const validated = await validateStoredAccountConnection(account);
  completed.set(input.state, { accountId: validated.id, completedAt: Date.now() });
  return validated;
}

export async function validateStoredAccountConnection(account: MailAccount): Promise<MailAccount> {
  try {
    const { testAccount } = await import('./mail.js');
    await testAccount(account);
    const connected: MailAccount = { ...account, status: 'connected', lastError: undefined };
    await updateStore((data) => {
      const current = data.accounts.find((item) => item.id === account.id);
      if (current) { current.status = 'connected'; current.lastError = undefined; }
    });
    return connected;
  } catch (error) {
    const lastError = error instanceof Error ? error.message : '邮箱连接验证失败';
    const failed: MailAccount = { ...account, status: 'error', lastError };
    await updateStore((data) => {
      const current = data.accounts.find((item) => item.id === account.id);
      if (current) { current.status = 'error'; current.lastError = lastError; }
    });
    return failed;
  }
}

async function refreshAccountSecret(account: MailAccount, secret: AccountSecret): Promise<AccountSecret> {
  if (!secret.oauthProvider || !secret.refreshToken) throw new Error('OAuth 授权已过期且没有刷新 Token，请重新连接邮箱');
  const config = providerConfig(secret.oauthProvider, account.provider);
  if (!config.configured) throw new Error(`OAuth Token 已过期。${config.configurationHint}`);
  const token = await tokenRequest(config, new URLSearchParams({ grant_type: 'refresh_token', refresh_token: secret.refreshToken }));
  const refreshed = tokenToSecret(config, token, secret.refreshToken);
  await updateStore(async (data) => {
    const current = data.accounts.find((item) => item.id === account.id);
    if (current) current.encryptedSecret = await encryptSecret(refreshed);
  });
  return refreshed;
}

export async function resolveAccountSecret(account: MailAccount): Promise<AccountSecret> {
  const secret = await decryptSecret(account.encryptedSecret);
  if (secret.authType !== 'oauth2' || !secret.expiresAt || new Date(secret.expiresAt).getTime() > Date.now() + 90_000) return secret;
  const existing = refreshes.get(account.id);
  if (existing) return existing;
  const operation = refreshAccountSecret(account, secret).finally(() => refreshes.delete(account.id));
  refreshes.set(account.id, operation);
  return operation;
}

export function oauthCallbackHtml(payload: { success: boolean; accountId?: string; message: string; warning?: string }) {
  const targetOrigin = process.env.FRONTEND_URL ?? 'http://localhost:5173';
  const serialized = JSON.stringify({ source: 'imail-oauth', ...payload }).replace(/</g, '\\u003c');
  return `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>iMail OAuth</title><style>body{font-family:system-ui;background:#edf3f1;color:#18302a;display:grid;place-items:center;min-height:100vh;margin:0}.box{background:white;padding:32px;border-radius:18px;box-shadow:0 20px 60px #163a3022;text-align:center;max-width:420px}h1{font-size:22px}p{color:#647b74;line-height:1.6}</style></head><body><div class="box"><h1>${payload.success ? payload.warning ? '授权已保存' : '邮箱已连接' : '连接未完成'}</h1><p>${payload.message.replace(/[<>&]/g, '')}</p><p>可以关闭此窗口并返回 iMail。</p></div><script>if(window.opener){window.opener.postMessage(${serialized},${JSON.stringify(targetOrigin)});setTimeout(()=>window.close(),900)}</script></body></html>`;
}
