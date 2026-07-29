import { resolveAccountSecret } from '../oauth.js';
import { readStore } from '../store.js';
import { smtpTransport } from './client.js';

export type SendMessageInput = {
  accountId: string;
  to: string[];
  cc?: string[];
  subject: string;
  text: string;
  html?: string;
};

export async function sendMessage(input: SendMessageInput) {
  const store = await readStore();
  const account = store.accounts.find((item) => item.id === input.accountId);
  if (!account) throw new Error('发件邮箱不存在');
  const secret = await resolveAccountSecret(account);
  const transport = smtpTransport(account, secret);
  const result = await transport.sendMail({
    from: { name: account.displayName, address: account.email },
    to: input.to, cc: input.cc, subject: input.subject, text: input.text, html: input.html,
  });
  return { messageId: result.messageId, accepted: result.accepted };
}
