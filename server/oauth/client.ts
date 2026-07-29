import { createRemoteJWKSet, jwtVerify, type JWTPayload } from 'jose';
import type { AccountSecret } from '../types.js';
import type { OAuthConfig } from './config.js';

export type OAuthTokenResponse = {
  access_token: string; refresh_token?: string; expires_in?: number; token_type?: string; scope?: string;
  id_token?: string; error?: string; error_description?: string;
};

export async function tokenRequest(config: OAuthConfig, parameters: URLSearchParams): Promise<OAuthTokenResponse> {
  parameters.set('client_id', config.clientId!);
  if (config.clientSecret && config.key !== 'yahoo') parameters.set('client_secret', config.clientSecret);
  const headers: Record<string, string> = { 'Content-Type': 'application/x-www-form-urlencoded', Accept: 'application/json' };
  if (config.key === 'yahoo' && config.clientSecret) headers.Authorization = `Basic ${Buffer.from(`${config.clientId}:${config.clientSecret}`).toString('base64')}`;
  const response = await fetch(config.tokenEndpoint, { method: 'POST', headers, body: parameters });
  const body = await response.json() as OAuthTokenResponse;
  if (!response.ok || body.error || !body.access_token) throw new Error(body.error_description || body.error || `OAuth Token 交换失败 (${response.status})`);
  return body;
}

export async function fetchIdentity(config: OAuthConfig, token: OAuthTokenResponse, nonce: string): Promise<{ email: string; name?: string }> {
  if (config.key === 'microsoft') {
    if (!token.id_token || !config.jwksUri) throw new Error('Microsoft 未返回 ID Token');
    const result = await jwtVerify(token.id_token, createRemoteJWKSet(new URL(config.jwksUri)), { audience: config.clientId });
    const payload = result.payload as JWTPayload & { preferred_username?: string; email?: string; name?: string; nonce?: string };
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

export function tokenToSecret(config: OAuthConfig, token: OAuthTokenResponse, previousRefreshToken?: string): AccountSecret {
  return {
    authType: 'oauth2', oauthProvider: config.key, accessToken: token.access_token,
    refreshToken: token.refresh_token || previousRefreshToken,
    expiresAt: new Date(Date.now() + Math.max(60, token.expires_in ?? 3600) * 1000).toISOString(),
    scopes: (token.scope || config.scopes.join(' ')).split(/\s+/).filter(Boolean), tokenType: token.token_type || 'Bearer',
  };
}
