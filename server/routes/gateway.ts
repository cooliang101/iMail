import { Router, type Request, type Response } from 'express';
import { z } from 'zod';
import { asyncRoute } from '../http/async-route.js';
import { requireDevToken } from '../http/dev-token.js';
import { gatewayNotFound } from '../http/errors.js';
import { publicGatewayMailbox } from '../http/presenters.js';
import { sendSchema } from '../http/schemas.js';
import { sendMessage } from '../mail.js';
import { readStore } from '../store.js';

export const gatewayRouter = Router();

gatewayRouter.get('/mailboxes', requireDevToken('accounts:read'), asyncRoute(async (_req, res) => {
  const token = res.locals.devToken;
  const data = await readStore();
  res.json({ mailboxes: data.accounts.filter((account) => token.accountIds.includes(account.id)).map(publicGatewayMailbox) });
}));

function messageQuery(req: Request) {
  const query = z.object({
    mailbox: z.string().email().optional(),
    limit: z.coerce.number().int().min(1).max(100).default(50),
    offset: z.coerce.number().int().min(0).default(0),
  }).strict().parse(req.query);
  const mailbox = req.params.mailbox ? z.string().email().parse(req.params.mailbox) : query.mailbox;
  return { ...query, mailbox: mailbox?.toLowerCase() };
}

async function listGatewayMessages(req: Request, res: Response) {
  const token = res.locals.devToken;
  const data = await readStore();
  const { mailbox, limit, offset } = messageQuery(req);
  const matchingAccount = mailbox ? data.accounts.find((account) => account.email.toLowerCase() === mailbox) : undefined;
  if (mailbox && !matchingAccount) { res.status(404).json({ error: '指定的邮箱不存在' }); return; }
  if (matchingAccount && !token.accountIds.includes(matchingAccount.id)) { res.status(403).json({ error: 'Token 无权访问这个邮箱' }); return; }
  const permittedIds = new Set(matchingAccount ? [matchingAccount.id] : token.accountIds);
  const accountsById = new Map(data.accounts.map((account) => [account.id, account.email]));
  const filtered = data.messages.filter((message) => permittedIds.has(message.accountId));
  const messages = filtered.slice(offset, offset + limit).map(({ accountId, mailbox: folder, uid: _uid, ...message }) => ({
    ...message, accountEmail: accountsById.get(accountId), folder,
  }));
  res.json({ messages, total: filtered.length, nextOffset: Math.min(offset + limit, filtered.length) });
}

gatewayRouter.get('/messages', requireDevToken('messages:read'), asyncRoute(listGatewayMessages));
gatewayRouter.get('/mailboxes/:mailbox/messages', requireDevToken('messages:read'), asyncRoute(listGatewayMessages));

gatewayRouter.post('/send', requireDevToken('messages:send'), asyncRoute(async (req, res) => {
  const token = res.locals.devToken;
  const input = sendSchema.omit({ accountId: true }).extend({ mailbox: z.string().email() }).parse(req.body);
  const data = await readStore();
  const account = data.accounts.find((item) => item.email.toLowerCase() === input.mailbox.toLowerCase());
  if (!account) { res.status(404).json({ error: '指定的发件邮箱不存在' }); return; }
  if (!token.accountIds.includes(account.id)) { res.status(403).json({ error: 'Token 无权使用这个发件箱' }); return; }
  const { mailbox: _mailbox, ...message } = input;
  res.status(201).json(await sendMessage({ ...message, accountId: account.id }));
}));

gatewayRouter.use(gatewayNotFound);
