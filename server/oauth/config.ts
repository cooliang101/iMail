import type { ProviderId } from '../types.js';

export type OAuthProviderKey = 'google' | 'microsoft' | 'yahoo';

export type OAuthConfig = {
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

function callbackBase() {
  return process.env.OAUTH_CALLBACK_BASE_URL ?? `http://localhost:${process.env.PORT ?? 8787}/api/oauth`;
}

export function providerConfig(key: OAuthProviderKey, accountProvider?: ProviderId): OAuthConfig {
  if (key === 'google') {
    const clientId = process.env.GOOGLE_OAUTH_CLIENT_ID;
    const clientSecret = process.env.GOOGLE_OAUTH_CLIENT_SECRET;
    return {
      key, clientId, clientSecret,
      redirectUri: process.env.GOOGLE_OAUTH_REDIRECT_URI ?? `${callbackBase()}/google/callback`,
      authorizationEndpoint: 'https://accounts.google.com/o/oauth2/v2/auth', tokenEndpoint: 'https://oauth2.googleapis.com/token',
      userInfoEndpoint: 'https://openidconnect.googleapis.com/v1/userinfo',
      scopes: ['openid', 'email', 'profile', 'https://mail.google.com/'], configured: Boolean(clientId && clientSecret),
      configurationHint: '配置 Google Desktop App 凭据中的 Client ID 与 Client Secret；桌面公共客户端仍使用 PKCE，Client Secret 不作为可保密凭据。',
    };
  }
  if (key === 'microsoft') {
    const clientId = process.env.MICROSOFT_OAUTH_CLIENT_ID;
    const tenant = accountProvider === 'hotmail' ? 'consumers' : 'common';
    return {
      key, clientId, clientSecret: process.env.MICROSOFT_OAUTH_CLIENT_SECRET,
      redirectUri: process.env.MICROSOFT_OAUTH_REDIRECT_URI ?? `${callbackBase()}/microsoft/callback`,
      authorizationEndpoint: `https://login.microsoftonline.com/${tenant}/oauth2/v2.0/authorize`,
      tokenEndpoint: `https://login.microsoftonline.com/${tenant}/oauth2/v2.0/token`,
      jwksUri: 'https://login.microsoftonline.com/common/discovery/v2.0/keys',
      scopes: ['openid', 'email', 'profile', 'offline_access', 'https://outlook.office.com/IMAP.AccessAsUser.All', 'https://outlook.office.com/SMTP.Send'],
      configured: Boolean(clientId),
      configurationHint: '配置 Microsoft Desktop App 的 OAuth Client ID；桌面公共客户端使用 PKCE，不需要 Client Secret。',
    };
  }
  const clientId = process.env.YAHOO_OAUTH_CLIENT_ID;
  const clientSecret = process.env.YAHOO_OAUTH_CLIENT_SECRET;
  const approved = process.env.YAHOO_MAIL_OAUTH_APPROVED === 'true';
  return {
    key, clientId, clientSecret,
    redirectUri: process.env.YAHOO_OAUTH_REDIRECT_URI ?? `${callbackBase()}/yahoo/callback`,
    authorizationEndpoint: 'https://api.login.yahoo.com/oauth2/request_auth', tokenEndpoint: 'https://api.login.yahoo.com/oauth2/get_token',
    userInfoEndpoint: 'https://api.login.yahoo.com/openid/v1/userinfo', scopes: ['openid', 'email', 'profile', 'mail-r', 'mail-w'],
    configured: Boolean(clientId && clientSecret && approved),
    configurationHint: 'Yahoo mail-r/mail-w 是受限权限。审核通过后配置 YAHOO_OAUTH_CLIENT_ID、YAHOO_OAUTH_CLIENT_SECRET 与 YAHOO_MAIL_OAUTH_APPROVED=true。',
  };
}

export function oauthKeyFor(provider: ProviderId): OAuthProviderKey | null {
  if (provider === 'gmail') return 'google';
  if (provider === 'outlook' || provider === 'hotmail') return 'microsoft';
  if (provider === 'yahoo') return 'yahoo';
  return null;
}

export function oauthProviderCatalog() {
  return (['google', 'microsoft', 'yahoo'] as const).map((key) => {
    const config = providerConfig(key);
    return { id: key, configured: config.configured, redirectUri: config.redirectUri, scopes: config.scopes, configurationHint: config.configurationHint };
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
