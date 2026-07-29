import type { DeveloperToken, MailAccount } from '../types.js';

export function publicAccount(account: MailAccount) {
  const { encryptedSecret: _secret, ...safe } = account;
  return { ...safe, authMethod: safe.authMethod ?? 'app-password' };
}

export function publicDeveloperToken(token: DeveloperToken, accounts: MailAccount[]) {
  const { tokenHash: _hash, accountIds, ...safe } = token;
  const emailsById = new Map(accounts.map((account) => [account.id, account.email]));
  const mailboxes = accountIds.map((id) => emailsById.get(id)).filter((email): email is string => Boolean(email));
  return { ...safe, mailboxes };
}

export function publicGatewayMailbox(account: MailAccount) {
  const { id: _id, encryptedSecret: _secret, settings: _settings, ...safe } = account;
  return safe;
}
