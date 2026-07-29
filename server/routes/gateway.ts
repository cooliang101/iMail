import { Router } from 'express';
import { z } from 'zod';
import { gatewayMessageQuerySchema, gatewaySendSchema } from '../gateway/contracts.js';
import { requireGatewayToken } from '../gateway/auth.js';
import { gatewayErrorHandler, gatewayNotFound, gatewayRequestContext } from '../gateway/errors.js';
import { assertGatewayAttachment, getGatewayMessage, getGatewaySendingAccount, listGatewayMailboxes, listGatewayMessages } from '../gateway/service.js';
import { asyncRoute } from '../http/async-route.js';
import { downloadAttachment, sendMessage } from '../mail.js';
import { readStore } from '../store.js';

export const gatewayRouter = Router();

gatewayRouter.use(gatewayRequestContext);
gatewayRouter.get('/health', (_req, res) => res.json({ service: 'imail-gateway', version: 'v1', ok: true }));

gatewayRouter.get('/mailboxes', requireGatewayToken('accounts:read'), asyncRoute(async (_req, res) => {
  res.json({ mailboxes: listGatewayMailboxes(await readStore(), res.locals.devToken) });
}));

gatewayRouter.get('/messages', requireGatewayToken('messages:read'), asyncRoute(async (req, res) => {
  const query = gatewayMessageQuerySchema.parse(req.query);
  res.json(listGatewayMessages(await readStore(), res.locals.devToken, query));
}));

gatewayRouter.get('/mailboxes/:mailbox/messages', requireGatewayToken('messages:read'), asyncRoute(async (req, res) => {
  const mailbox = z.string().email().parse(req.params.mailbox);
  const query = gatewayMessageQuerySchema.parse({ ...req.query, mailbox });
  res.json(listGatewayMessages(await readStore(), res.locals.devToken, query));
}));

gatewayRouter.get('/messages/:messageId', requireGatewayToken('messages:read'), asyncRoute(async (req, res) => {
  const messageId = z.string().min(1).max(200).parse(req.params.messageId);
  res.json({ message: getGatewayMessage(await readStore(), res.locals.devToken, messageId).response });
}));

gatewayRouter.get('/messages/:messageId/attachments/:index', requireGatewayToken('messages:read'), asyncRoute(async (req, res) => {
  const messageId = z.string().min(1).max(200).parse(req.params.messageId);
  const index = z.coerce.number().int().min(0).parse(req.params.index);
  assertGatewayAttachment(await readStore(), res.locals.devToken, messageId, index);
  const attachment = await downloadAttachment(messageId, index);
  const filename = attachment.filename.replace(/[\r\n"\\]/g, '_');
  res.setHeader('Content-Type', attachment.contentType || 'application/octet-stream');
  res.setHeader('Content-Disposition', `attachment; filename*=UTF-8''${encodeURIComponent(filename)}`);
  res.send(attachment.content);
}));

gatewayRouter.post('/send', requireGatewayToken('messages:send'), asyncRoute(async (req, res) => {
  const input = gatewaySendSchema.parse(req.body);
  const data = await readStore();
  const account = getGatewaySendingAccount(data, res.locals.devToken, input.mailbox);
  const { mailbox: _mailbox, ...message } = input;
  const delivery = await sendMessage({ ...message, accountId: account.id });
  res.status(201).json({ delivery });
}));

gatewayRouter.use(gatewayErrorHandler);
gatewayRouter.use(gatewayNotFound);
