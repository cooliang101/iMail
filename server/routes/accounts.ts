import { Router } from 'express';
import { accountById, createAccount, removeAccount, replaceAccountPassword, updateAccountMetadata, updateAccountProxy } from '../domain/accounts.js';
import { accountMetadataSchema, accountProxyUpdateSchema, appPasswordSchema } from '../domain/schemas.js';
import { asyncRoute } from '../http/async-route.js';
import { publicAccount } from '../http/presenters.js';
import { accountSchema, mailboxRoleSchema } from '../http/schemas.js';
import { beginOAuthReconnect, validateStoredAccountConnection } from '../oauth.js';
import { readStore } from '../store.js';
import type { MailboxRole } from '../types.js';
import { getSyncStore } from '../sync/store.js';
import { canonicalSyncTarget } from '../mail/mailbox-role.js';
import { z } from 'zod';
import { recordRequestSecurityEvent } from '../auth/http.js';

export const accountsRouter = Router();

accountsRouter.get('/accounts', asyncRoute(async (_req, res) => {
  const data = await readStore();
  res.json({ accounts: data.accounts.map(publicAccount) });
}));

accountsRouter.post('/accounts/:id/oauth/reconnect', asyncRoute(async (req, res) => {
  const accountId = String(req.params.id);
  const result = await beginOAuthReconnect(await accountById(accountId));
  recordRequestSecurityEvent(req, res, 'account.oauth-reconnect-started', { accountId });
  res.json(result);
}));

accountsRouter.post('/accounts/:id/connection-test', asyncRoute(async (req, res) => {
  const checked = await validateStoredAccountConnection(await accountById(String(req.params.id)));
  res.json({ account: publicAccount(checked) });
}));

accountsRouter.put('/accounts/:id/credential', asyncRoute(async (req, res) => {
  const input = z.object({ password: appPasswordSchema }).parse(req.body);
  const accountId = String(req.params.id);
  const account = await replaceAccountPassword(accountId, input.password);
  recordRequestSecurityEvent(req, res, 'account.credential-updated', { accountId });
  res.json({ account: publicAccount(account) });
}));

accountsRouter.put('/accounts/:id/proxy', asyncRoute(async (req, res) => {
  const input = accountProxyUpdateSchema.parse(req.body);
  const accountId = String(req.params.id);
  const account = await updateAccountProxy(accountId, input);
  recordRequestSecurityEvent(req, res, 'account.proxy-updated', { accountId, enabled: String(Boolean(input.enabled)) });
  res.json({ account: publicAccount(account) });
}));

accountsRouter.post('/accounts', asyncRoute(async (req, res) => {
  const input = accountSchema.parse(req.body);
  const account = await createAccount(input);
  recordRequestSecurityEvent(req, res, 'account.created', { accountId: account.id, provider: account.provider });
  res.status(201).json({ account: publicAccount(account) });
}));

accountsRouter.delete('/accounts/:id', asyncRoute(async (req, res) => {
  const accountId = String(req.params.id);
  await removeAccount(accountId);
  recordRequestSecurityEvent(req, res, 'account.removed', { accountId });
  res.status(204).end();
}));

accountsRouter.patch('/accounts/:id', asyncRoute(async (req, res) => {
  const input = accountMetadataSchema.parse(req.body);
  res.json({ account: publicAccount(await updateAccountMetadata(String(req.params.id), input)) });
}));

function queuedResult(accountId: string, mailboxRole: MailboxRole = 'inbox', mailbox?: string) {
  const job = getSyncStore().enqueueJob({ accountId, mailboxRole, mailbox, reason: 'manual', priority: 100 });
  return { synced: 0, queued: true, jobId: job.id };
}

async function accountExists(accountId: string) { return (await readStore()).accounts.some((account) => account.id === accountId); }

accountsRouter.post('/accounts/:id/sync', asyncRoute(async (req, res) => {
  const accountId = String(req.params.id);
  if (!(await readStore()).accounts.some((account) => account.id === accountId)) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  res.json(queuedResult(accountId));
}));

accountsRouter.post('/accounts/:id/mailboxes/:role/sync', asyncRoute(async (req, res) => {
  const role = mailboxRoleSchema.parse(req.params.role);
  const accountId = String(req.params.id);
  if (!await accountExists(accountId)) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  res.json(queuedResult(accountId, role));
}));

accountsRouter.post('/accounts/:id/mailboxes/sync', asyncRoute(async (req, res) => {
  const input = z.object({ mailbox: z.string().trim().min(1).max(500) }).parse(req.body);
  const accountId = String(req.params.id);
  const account = (await readStore()).accounts.find((item) => item.id === accountId);
  if (!account) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  const target = canonicalSyncTarget(account, 'custom', input.mailbox);
  res.json(queuedResult(accountId, target.mailboxRole, target.mailbox));
}));

accountsRouter.post('/mailboxes/:role/sync', asyncRoute(async (req, res) => {
  const role = mailboxRoleSchema.parse(req.params.role);
  const { accounts } = await readStore();
  const results = accounts.map((account) => queuedResult(account.id, role));
  res.json({ results: results.map((result, index) => ({ accountId: accounts[index].id, status: 'fulfilled', ...result })) });
}));

accountsRouter.post('/sync', asyncRoute(async (_req, res) => {
  const { accounts } = await readStore();
  const results = accounts.map((account) => queuedResult(account.id));
  res.json({ results: results.map((result, index) => ({ accountId: accounts[index].id, status: 'fulfilled', ...result })) });
}));
