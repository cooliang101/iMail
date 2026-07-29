import crypto from 'node:crypto';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import cors from 'cors';
import express, { type NextFunction, type Request, type Response } from 'express';
import { z } from 'zod';
import { encryptSecret } from './crypto.js';
import { sendMessage, syncAccount, testAccount, updateRemoteMessageFlags } from './mail.js';
import { beginOAuth, beginOAuthReconnect, completeOAuth, oauthCallbackHtml, oauthProviderCatalog, validateStoredAccountConnection, type OAuthProviderKey } from './oauth.js';
import { settingsFor } from './providers.js';
import { getCachedMessage, getMessageStats, listCachedMessages, readStore, updateStore } from './store.js';
import { authenticateToken, issueToken } from './tokens.js';
import type { MailAccount, MailSettings, ProviderId, TokenScope } from './types.js';

export const app = express();
const origins = (process.env.CORS_ORIGIN ?? 'http://localhost:5173').split(',').map((item) => item.trim());
app.use(cors({ origin: origins }));
app.use(express.json({ limit: '2mb' }));

const asyncRoute = (handler: (req: Request, res: Response, next: NextFunction) => Promise<unknown>) =>
  (req: Request, res: Response, next: NextFunction) => void handler(req, res, next).catch(next);

const providerSchema = z.enum(['outlook', 'gmail', 'qq', 'yahoo', 'hotmail', 'icloud', 'custom']);
const settingsSchema = z.object({
  imapHost: z.string().min(1), imapPort: z.number().int().min(1).max(65535), imapSecure: z.boolean(),
  smtpHost: z.string().min(1), smtpPort: z.number().int().min(1).max(65535), smtpSecure: z.boolean(),
});
const accountSchema = z.object({
  provider: providerSchema,
  email: z.string().email(),
  displayName: z.string().min(1).max(80),
  group: z.string().min(1).max(40).default('个人'),
  color: z.string().regex(/^#[0-9a-fA-F]{6}$/).default('#17a887'),
  password: z.string().optional(),
  accessToken: z.string().optional(),
  settings: settingsSchema.optional(),
}).refine((value) => Boolean(value.password || value.accessToken), '请填写应用专用密码或 OAuth Access Token');

function publicAccount(account: MailAccount) {
  const { encryptedSecret: _secret, ...safe } = account;
  return { ...safe, authMethod: safe.authMethod ?? 'app-password' };
}

app.get('/api/health', (_req, res) => res.json({ ok: true, service: 'imail' }));

app.get('/api/providers', (_req, res) => res.json({
  providers: [
    { id: 'outlook', name: 'Outlook / Microsoft 365', hint: '使用 Microsoft 安全登录', authMode: 'oauth2', oauthProvider: 'microsoft', fallbackAuthMode: null },
    { id: 'gmail', name: 'Gmail', hint: '使用 Google 安全登录', authMode: 'oauth2', oauthProvider: 'google', fallbackAuthMode: 'app-password' },
    { id: 'qq', name: 'QQ 邮箱', hint: 'QQ 未公开邮件 OAuth，请使用授权码', authMode: 'authorization-code', oauthProvider: null, helpUrl: 'https://help.mail.qq.com/detail/106/985' },
    { id: 'yahoo', name: 'Yahoo', hint: 'OAuth 需要 Yahoo Mail 接入审核；未审核可使用第三方应用密码', authMode: 'oauth2', oauthProvider: 'yahoo', fallbackAuthMode: 'app-password', helpUrl: 'https://login.yahoo.com/account/security' },
    { id: 'hotmail', name: 'Hotmail / Outlook.com', hint: '使用 Microsoft 个人账户安全登录', authMode: 'oauth2', oauthProvider: 'microsoft', oauthTenant: 'consumers', fallbackAuthMode: null },
    { id: 'icloud', name: 'iCloud', hint: '普通跨平台客户端使用 Apple 应用专用密码', authMode: 'app-password', oauthProvider: null, helpUrl: 'https://account.apple.com/account/manage' },
    { id: 'custom', name: '其他邮箱', hint: '自定义 IMAP / SMTP', authMode: 'custom' },
  ],
  oauth: oauthProviderCatalog(),
}));

const oauthStartSchema = z.object({
  provider: z.enum(['outlook', 'gmail', 'yahoo', 'hotmail']),
  displayName: z.string().max(80).optional(),
  group: z.string().min(1).max(40).default('个人'),
  color: z.string().regex(/^#[0-9a-fA-F]{6}$/).default('#168f78'),
});
app.post('/api/oauth/start', asyncRoute(async (req, res) => {
  res.json(await beginOAuth(oauthStartSchema.parse(req.body)));
}));

for (const providerKey of ['google', 'microsoft', 'yahoo'] as const) {
  app.get(`/api/oauth/${providerKey}/callback`, asyncRoute(async (req, res) => {
    try {
      const account = await completeOAuth({
        providerKey: providerKey as OAuthProviderKey,
        state: typeof req.query.state === 'string' ? req.query.state : undefined,
        code: typeof req.query.code === 'string' ? req.query.code : undefined,
        error: typeof req.query.error === 'string' ? req.query.error : undefined,
        errorDescription: typeof req.query.error_description === 'string' ? req.query.error_description : undefined,
      });
      const warning = account.status === 'error' ? account.lastError : undefined;
      res.type('html').send(oauthCallbackHtml({
        success: true,
        accountId: account.id,
        warning,
        message: warning ? `${account.email} 的 OAuth 授权已安全保存。邮件连接暂时失败，iMail 将保留授权供后续重试。` : `${account.email} 已通过 OAuth 安全连接。`,
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'OAuth 登录失败';
      const correlationId = typeof req.query.correlation_id === 'string' ? req.query.correlation_id : undefined;
      const traceId = typeof req.query.trace_id === 'string' ? req.query.trace_id : undefined;
      console.error(`[OAuth ${providerKey}] ${message}`, { correlationId, traceId, error });
      res.status(400).type('html').send(oauthCallbackHtml({ success: false, message }));
    }
  }));
}

app.get('/api/accounts', asyncRoute(async (_req, res) => {
  const data = await readStore();
  res.json({ accounts: data.accounts.map(publicAccount) });
}));

app.post('/api/accounts/:id/oauth/reconnect', asyncRoute(async (req, res) => {
  const data = await readStore();
  const account = data.accounts.find((item) => item.id === req.params.id);
  if (!account) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  res.json(await beginOAuthReconnect(account));
}));

app.post('/api/accounts/:id/connection-test', asyncRoute(async (req, res) => {
  const data = await readStore();
  const account = data.accounts.find((item) => item.id === req.params.id);
  if (!account) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  const checked = await validateStoredAccountConnection(account);
  res.json({ account: publicAccount(checked) });
}));

app.put('/api/accounts/:id/credential', asyncRoute(async (req, res) => {
  const input = z.object({ password: z.string().min(1, '请填写新的授权码或应用专用密码').max(512) }).parse(req.body);
  const data = await readStore();
  const account = data.accounts.find((item) => item.id === req.params.id);
  if (!account) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  if (account.authMethod === 'oauth2') { res.status(409).json({ error: 'OAuth 邮箱请使用重新授权' }); return; }
  const candidate: MailAccount = {
    ...account,
    encryptedSecret: await encryptSecret({ authType: 'app-password', password: input.password }),
    authMethod: 'app-password',
    status: 'syncing',
    lastError: undefined,
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

app.post('/api/accounts', asyncRoute(async (req, res) => {
  const input = accountSchema.parse(req.body);
  const settings = settingsFor(input.provider as ProviderId, input.settings as MailSettings | undefined);
  const account: MailAccount = {
    id: crypto.randomUUID(),
    provider: input.provider,
    email: input.email.toLowerCase(),
    displayName: input.displayName,
    group: input.group,
    color: input.color,
    settings,
    encryptedSecret: await encryptSecret({ authType: input.password ? 'app-password' : 'oauth2', password: input.password, accessToken: input.accessToken }),
    authMethod: input.password ? 'app-password' : 'oauth2',
    createdAt: new Date().toISOString(),
    status: 'connected',
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

app.delete('/api/accounts/:id', asyncRoute(async (req, res) => {
  await updateStore((data) => {
    data.accounts = data.accounts.filter((item) => item.id !== req.params.id);
    data.messages = data.messages.filter((item) => item.accountId !== req.params.id);
    data.tokens.forEach((token) => { token.accountIds = token.accountIds.filter((id) => id !== req.params.id); });
  });
  res.status(204).end();
}));

app.post('/api/accounts/:id/sync', asyncRoute(async (req, res) => res.json(await syncAccount(String(req.params.id)))));
app.post('/api/sync', asyncRoute(async (_req, res) => {
  const { accounts } = await readStore();
  const results = await Promise.allSettled(accounts.map((account) => syncAccount(account.id)));
  res.json({ results: results.map((result, index) => ({ accountId: accounts[index].id, status: result.status, ...(result.status === 'fulfilled' ? result.value : { error: result.reason instanceof Error ? result.reason.message : '同步失败' }) })) });
}));

app.get('/api/messages', asyncRoute(async (req, res) => {
  const input = z.object({
    accountId: z.string().optional(), group: z.string().optional(), q: z.string().max(200).optional(),
    unread: z.enum(['true', 'false']).optional(), flagged: z.enum(['true', 'false']).optional(), hasAttachments: z.enum(['true', 'false']).optional(),
    limit: z.coerce.number().int().min(1).max(100).default(60), offset: z.coerce.number().int().min(0).default(0),
  }).parse(req.query);
  const result = await listCachedMessages({
    accountId: input.accountId, group: input.group, query: input.q,
    unread: input.unread === 'true', flagged: input.flagged === 'true', hasAttachments: input.hasAttachments === 'true', limit: input.limit, offset: input.offset,
  });
  const messages = result.messages.map(({ text: _text, html: _html, ...summary }) => summary);
  res.json({ messages, total: result.total, nextOffset: input.offset + messages.length, hasMore: input.offset + messages.length < result.total });
}));

app.get('/api/message-stats', asyncRoute(async (_req, res) => {
  res.json(await getMessageStats());
}));

app.get('/api/messages/:id', asyncRoute(async (req, res) => {
  const message = await getCachedMessage(String(req.params.id));
  if (!message) { res.status(404).json({ error: '邮件不存在' }); return; }
  res.json({ message });
}));

app.patch('/api/messages/:id', asyncRoute(async (req, res) => {
  const input = z.object({ unread: z.boolean().optional(), flagged: z.boolean().optional() }).parse(req.body);
  await updateRemoteMessageFlags(String(req.params.id), input);
  let updated;
  await updateStore((data) => {
    updated = data.messages.find((item) => item.id === req.params.id);
    if (!updated) throw new Error('邮件不存在');
    Object.assign(updated, input);
  });
  res.json({ message: updated });
}));

const sendSchema = z.object({ accountId: z.string().uuid(), to: z.array(z.string().email()).min(1), cc: z.array(z.string().email()).optional(), subject: z.string().min(1), text: z.string().min(1), html: z.string().optional() });
app.post('/api/send', asyncRoute(async (req, res) => res.status(201).json(await sendMessage(sendSchema.parse(req.body)))));

const tokenSchema = z.object({
  name: z.string().min(1).max(80),
  scopes: z.array(z.enum(['messages:read', 'messages:send', 'accounts:read'])).min(1),
  accountIds: z.array(z.string().uuid()).min(1),
  ttlSeconds: z.number().int().min(300).max(7 * 24 * 3600),
});
app.get('/api/developer-tokens', asyncRoute(async (_req, res) => {
  const data = await readStore();
  res.json({ tokens: data.tokens.map(({ tokenHash: _hash, ...token }) => token).sort((a, b) => b.createdAt.localeCompare(a.createdAt)) });
}));
app.post('/api/developer-tokens', asyncRoute(async (req, res) => {
  const input = tokenSchema.parse(req.body);
  const data = await readStore();
  if (input.accountIds.some((id) => !data.accounts.some((account) => account.id === id))) throw new Error('包含不存在的邮箱账户');
  const result = await issueToken(input as { name: string; scopes: TokenScope[]; accountIds: string[]; ttlSeconds: number });
  res.status(201).json({ token: result.raw, detail: { ...result.token, tokenHash: undefined } });
}));
app.delete('/api/developer-tokens/:id', asyncRoute(async (req, res) => {
  await updateStore((data) => { data.tokens = data.tokens.filter((item) => item.id !== req.params.id); });
  res.status(204).end();
}));

function bearer(req: Request) { return req.headers.authorization?.replace(/^Bearer\s+/i, ''); }
function requireDevToken(scope: TokenScope) {
  return asyncRoute(async (req, res, next) => {
    const token = await authenticateToken(bearer(req), scope);
    if (!token) { res.status(401).json({ error: 'Token 无效、已过期或缺少权限' }); return; }
    res.locals.devToken = token;
    next();
  });
}

app.get('/api/dev/v1/accounts', requireDevToken('accounts:read'), asyncRoute(async (_req, res) => {
  const token = res.locals.devToken;
  const data = await readStore();
  res.json({ accounts: data.accounts.filter((account) => token.accountIds.includes(account.id)).map(publicAccount) });
}));
function devAccountSelector(req: Request) {
  return {
    account: typeof req.params.account === 'string' ? req.params.account : typeof req.query.account === 'string' ? req.query.account : undefined,
    accountId: typeof req.query.accountId === 'string' ? req.query.accountId : undefined,
    accountEmail: typeof req.query.accountEmail === 'string' ? req.query.accountEmail.toLowerCase() : undefined,
    provider: typeof req.query.provider === 'string' ? req.query.provider : undefined,
  };
}

async function devMessages(req: Request, res: Response) {
  const token = res.locals.devToken;
  const data = await readStore();
  const selector = devAccountSelector(req);
  const matchingAccounts = data.accounts.filter((account) =>
    (!selector.account || account.id === selector.account || account.email.toLowerCase() === selector.account.toLowerCase()) &&
    (!selector.accountId || account.id === selector.accountId) &&
    (!selector.accountEmail || account.email.toLowerCase() === selector.accountEmail) &&
    (!selector.provider || account.provider === selector.provider));
  const hasSelector = Boolean(selector.account || selector.accountId || selector.accountEmail || selector.provider);
  if (hasSelector && matchingAccounts.length === 0) { res.status(404).json({ error: '指定的邮箱不存在' }); return; }
  const requestedAccounts = matchingAccounts.filter((account) => token.accountIds.includes(account.id));
  if (hasSelector && requestedAccounts.length === 0) { res.status(403).json({ error: 'Token 无权访问这个邮箱' }); return; }
  const permittedIds = new Set(hasSelector ? requestedAccounts.map((account) => account.id) : token.accountIds);
  const limit = Math.min(Math.max(Number(req.query.limit) || 50, 1), 100);
  const offset = Math.max(Number(req.query.offset) || 0, 0);
  const filtered = data.messages.filter((message) => permittedIds.has(message.accountId));
  res.json({ messages: filtered.slice(offset, offset + limit), total: filtered.length, nextOffset: Math.min(offset + limit, filtered.length) });
}

app.get('/api/dev/v1/messages', requireDevToken('messages:read'), asyncRoute(devMessages));
app.get('/api/dev/v1/accounts/:account/messages', requireDevToken('messages:read'), asyncRoute(devMessages));
app.post('/api/dev/v1/send', requireDevToken('messages:send'), asyncRoute(async (req, res) => {
  const token = res.locals.devToken;
  const input = sendSchema.omit({ accountId: true }).extend({
    accountId: z.string().uuid().optional(), accountEmail: z.string().email().optional(),
  }).refine((value) => Boolean(value.accountId || value.accountEmail), '请提供 accountId 或 accountEmail').parse(req.body);
  const data = await readStore();
  const account = data.accounts.find((item) => (!input.accountId || item.id === input.accountId) && (!input.accountEmail || item.email.toLowerCase() === input.accountEmail.toLowerCase()));
  if (!account) { res.status(404).json({ error: '指定的发件邮箱不存在' }); return; }
  if (!token.accountIds.includes(account.id)) { res.status(403).json({ error: 'Token 无权使用这个发件箱' }); return; }
  const { accountEmail: _accountEmail, ...message } = input;
  res.status(201).json(await sendMessage({ ...message, accountId: account.id }));
}));

app.use((error: unknown, _req: Request, res: Response, _next: NextFunction) => {
  if (error instanceof z.ZodError) { res.status(400).json({ error: error.issues.map((issue) => issue.message).join('；') }); return; }
  const message = error instanceof Error ? error.message : '服务发生未知错误';
  console.error(error);
  res.status(500).json({ error: message });
});

const port = Number(process.env.PORT ?? 8787);
const host = process.env.HOST ?? '127.0.0.1';
export function startServer() {
  return app.listen(port, host, () => console.log(`iMail API running at http://${host}:${port}`));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) startServer();
