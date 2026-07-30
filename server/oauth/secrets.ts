import { decryptSecret, encryptSecret } from '../crypto.js';
import { setAccountEncryptedSecret, setAccountSyncStatus } from '../store.js';
import type { AccountSecret, MailAccount } from '../types.js';
import { tokenRequest, tokenToSecret } from './client.js';
import { providerConfig } from './config.js';

const refreshes = new Map<string, Promise<AccountSecret>>();

export async function validateStoredAccountConnection(account: MailAccount): Promise<MailAccount> {
  try {
    const { testAccount } = await import('../mail.js');
    await testAccount(account);
    const connected: MailAccount = { ...account, status: 'connected', lastError: undefined };
    await setAccountSyncStatus(account.id, 'connected');
    return connected;
  } catch (error) {
    const lastError = error instanceof Error ? error.message : '邮箱连接验证失败';
    const failed: MailAccount = { ...account, status: 'error', lastError };
    await setAccountSyncStatus(account.id, 'error', lastError);
    return failed;
  }
}

async function refreshAccountSecret(account: MailAccount, secret: AccountSecret): Promise<AccountSecret> {
  if (!secret.oauthProvider || !secret.refreshToken) throw new Error('OAuth 授权已过期且没有刷新 Token，请重新连接邮箱');
  const config = providerConfig(secret.oauthProvider, account.provider);
  if (!config.configured) throw new Error(`OAuth Token 已过期。${config.configurationHint}`);
  const token = await tokenRequest(config, new URLSearchParams({ grant_type: 'refresh_token', refresh_token: secret.refreshToken }));
  const refreshed = tokenToSecret(config, token, secret.refreshToken);
  await setAccountEncryptedSecret(account.id, await encryptSecret(refreshed));
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
