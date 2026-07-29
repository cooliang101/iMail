import crypto from 'node:crypto';
import { Router } from 'express';
import { encryptSecret } from '../crypto.js';
import { asyncRoute } from '../http/async-route.js';
import { publicAccount } from '../http/presenters.js';
import { accountSchema, mailboxRoleSchema, workspaceIconSchema } from '../http/schemas.js';
import { syncAccount, testAccount } from '../mail.js';
import { beginOAuthReconnect, validateStoredAccountConnection } from '../oauth.js';
import { settingsFor } from '../providers.js';
import { readStore, updateStore } from '../store.js';
import type { MailAccount, MailSettings, ProviderId } from '../types.js';
import { z } from 'zod';

export const accountsRouter = Router();

accountsRouter.get('/accounts', asyncRoute(async (_req, res) => {
  const data = await readStore();
  res.json({ accounts: data.accounts.map(publicAccount) });
}));

accountsRouter.post('/accounts/:id/oauth/reconnect', asyncRoute(async (req, res) => {
  const data = await readStore();
  const account = data.accounts.find((item) => item.id === req.params.id);
  if (!account) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  res.json(await beginOAuthReconnect(account));
}));

accountsRouter.post('/accounts/:id/connection-test', asyncRoute(async (req, res) => {
  const data = await readStore();
  const account = data.accounts.find((item) => item.id === req.params.id);
  if (!account) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  const checked = await validateStoredAccountConnection(account);
  res.json({ account: publicAccount(checked) });
}));

accountsRouter.put('/accounts/:id/credential', asyncRoute(async (req, res) => {
  const input = z.object({ password: z.string().min(1, '请填写新的授权码或应用专用密码').max(512) }).parse(req.body);
  const data = await readStore();
  const account = data.accounts.find((item) => item.id === req.params.id);
  if (!account) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  if (account.authMethod === 'oauth2') { res.status(409).json({ error: 'OAuth 邮箱请使用重新授权' }); return; }
  const candidate: MailAccount = {
    ...account,
    encryptedSecret: await encryptSecret({ authType: 'app-password', password: input.password }),
    authMethod: 'app-password', status: 'syncing', lastError: undefined,
  };
  await testAccount(candidate);
  const connected: MailAccount = { ...candidate, status: 'connected' };
  await updateStore((store) => {
    const index = store.accounts.findIndex((item) => item.id === account.id);
    if (index < 0) throw new Error('邮箱账户已被移除');
    store.accounts[index] = connected;
  });
  res.json({ account: publicAccount(connected) });
}));

accountsRouter.post('/accounts', asyncRoute(async (req, res) => {
  const input = accountSchema.parse(req.body);
  const settings = settingsFor(input.provider as ProviderId, input.settings as MailSettings | undefined);
  const account: MailAccount = {
    id: crypto.randomUUID(), provider: input.provider, email: input.email.toLowerCase(), displayName: input.displayName,
    group: input.group, groupIcon: input.groupIcon, color: input.color, settings,
    encryptedSecret: await encryptSecret({ authType: input.password ? 'app-password' : 'oauth2', password: input.password, accessToken: input.accessToken }),
    authMethod: input.password ? 'app-password' : 'oauth2', createdAt: new Date().toISOString(), status: 'connected',
  };
  const existing = await readStore();
  if (existing.accounts.some((item) => item.email === account.email)) throw new Error('这个邮箱已经添加');
  await testAccount(account);
  await updateStore((data) => {
    if (data.accounts.some((item) => item.email === account.email)) throw new Error('这个邮箱刚刚被其他操作添加');
    data.accounts.push(account);
  });
  res.status(201).json({ account: publicAccount(account) });
}));

accountsRouter.delete('/accounts/:id', asyncRoute(async (req, res) => {
  await updateStore((data) => {
    data.accounts = data.accounts.filter((item) => item.id !== req.params.id);
    data.messages = data.messages.filter((item) => item.accountId !== req.params.id);
    data.tokens.forEach((token) => { token.accountIds = token.accountIds.filter((id) => id !== req.params.id); });
  });
  res.status(204).end();
}));

accountsRouter.patch('/accounts/:id', asyncRoute(async (req, res) => {
  const input = z.object({ displayName: z.string().trim().min(1).max(80).optional(), group: z.string().trim().min(1).max(40).optional(), groupIcon: workspaceIconSchema.optional(), color: z.string().regex(/^#[0-9a-fA-F]{6}$/).optional() }).parse(req.body);
  let updated: MailAccount | undefined;
  await updateStore((data) => {
    updated = data.accounts.find((item) => item.id === req.params.id);
    if (!updated) throw new Error('邮箱账户不存在');
    Object.assign(updated, input);
  });
  res.json({ account: publicAccount(updated!) });
}));

accountsRouter.post('/accounts/:id/sync', asyncRoute(async (req, res) => res.json(await syncAccount(String(req.params.id)))));

accountsRouter.post('/accounts/:id/mailboxes/:role/sync', asyncRoute(async (req, res) => {
  const role = mailboxRoleSchema.parse(req.params.role);
  res.json(await syncAccount(String(req.params.id), role));
}));

accountsRouter.post('/accounts/:id/mailboxes/sync', asyncRoute(async (req, res) => {
  const input = z.object({ mailbox: z.string().trim().min(1).max(500) }).parse(req.body);
  res.json(await syncAccount(String(req.params.id), 'custom', input.mailbox));
}));

accountsRouter.post('/mailboxes/:role/sync', asyncRoute(async (req, res) => {
  const role = mailboxRoleSchema.parse(req.params.role);
  const { accounts } = await readStore();
  const results = await Promise.allSettled(accounts.map((account) => syncAccount(account.id, role)));
  res.json({ results: results.map((result, index) => ({ accountId: accounts[index].id, status: result.status, ...(result.status === 'fulfilled' ? result.value : { error: result.reason instanceof Error ? result.reason.message : '同步失败' }) })) });
}));

accountsRouter.post('/sync', asyncRoute(async (_req, res) => {
  const { accounts } = await readStore();
  const results = await Promise.allSettled(accounts.map((account) => syncAccount(account.id)));
  res.json({ results: results.map((result, index) => ({ accountId: accounts[index].id, status: result.status, ...(result.status === 'fulfilled' ? result.value : { error: result.reason instanceof Error ? result.reason.message : '同步失败' }) })) });
}));
