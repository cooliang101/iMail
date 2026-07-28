import crypto from 'node:crypto';
import cors from 'cors';
import express, { type NextFunction, type Request, type Response } from 'express';
import { z } from 'zod';
import { encryptSecret } from './crypto.js';
import { sendMessage, syncAccount, testAccount } from './mail.js';
import { settingsFor } from './providers.js';
import { readStore, updateStore } from './store.js';
import { authenticateToken, issueToken } from './tokens.js';
import type { MailAccount, MailSettings, ProviderId, TokenScope } from './types.js';

const app = express();
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
  return safe;
}

app.get('/api/health', (_req, res) => res.json({ ok: true, service: 'relaybox' }));

app.get('/api/providers', (_req, res) => res.json({
  providers: [
    { id: 'outlook', name: 'Outlook', hint: 'Microsoft 365 / 工作邮箱' },
    { id: 'gmail', name: 'Gmail', hint: '推荐使用应用专用密码' },
    { id: 'qq', name: 'QQ 邮箱', hint: '使用邮箱授权码' },
    { id: 'yahoo', name: 'Yahoo', hint: '使用应用密码' },
    { id: 'hotmail', name: 'Hotmail', hint: 'Microsoft 个人邮箱' },
    { id: 'icloud', name: 'iCloud', hint: '使用 Apple 应用专用密码' },
    { id: 'custom', name: '其他邮箱', hint: '自定义 IMAP / SMTP' },
  ],
}));

app.get('/api/accounts', asyncRoute(async (_req, res) => {
  const data = await readStore();
  res.json({ accounts: data.accounts.map(publicAccount) });
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
    encryptedSecret: await encryptSecret({ password: input.password, accessToken: input.accessToken }),
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
  const data = await readStore();
  const accountId = typeof req.query.accountId === 'string' ? req.query.accountId : undefined;
  const group = typeof req.query.group === 'string' ? req.query.group : undefined;
  const query = typeof req.query.q === 'string' ? req.query.q.toLowerCase() : '';
  const unread = req.query.unread === 'true';
  const groupIds = group ? new Set(data.accounts.filter((account) => account.group === group).map((account) => account.id)) : null;
  const messages = data.messages.filter((message) =>
    (!accountId || message.accountId === accountId) &&
    (!groupIds || groupIds.has(message.accountId)) &&
    (!unread || message.unread) &&
    (!query || `${message.subject} ${message.from.name} ${message.from.address} ${message.preview}`.toLowerCase().includes(query))
  );
  res.json({ messages, total: messages.length });
}));

app.patch('/api/messages/:id', asyncRoute(async (req, res) => {
  const input = z.object({ unread: z.boolean().optional(), flagged: z.boolean().optional() }).parse(req.body);
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
app.get('/api/dev/v1/messages', requireDevToken('messages:read'), asyncRoute(async (req, res) => {
  const token = res.locals.devToken;
  const data = await readStore();
  const accountId = typeof req.query.accountId === 'string' ? req.query.accountId : undefined;
  if (accountId && !token.accountIds.includes(accountId)) { res.status(403).json({ error: 'Token 无权访问这个邮箱' }); return; }
  const limit = Math.min(Number(req.query.limit) || 50, 100);
  const messages = data.messages.filter((message) => token.accountIds.includes(message.accountId) && (!accountId || message.accountId === accountId)).slice(0, limit);
  res.json({ messages, total: messages.length });
}));
app.post('/api/dev/v1/send', requireDevToken('messages:send'), asyncRoute(async (req, res) => {
  const token = res.locals.devToken;
  const input = sendSchema.parse(req.body);
  if (!token.accountIds.includes(input.accountId)) { res.status(403).json({ error: 'Token 无权使用这个发件箱' }); return; }
  res.status(201).json(await sendMessage(input));
}));

app.use((error: unknown, _req: Request, res: Response, _next: NextFunction) => {
  if (error instanceof z.ZodError) { res.status(400).json({ error: error.issues.map((issue) => issue.message).join('；') }); return; }
  const message = error instanceof Error ? error.message : '服务发生未知错误';
  console.error(error);
  res.status(500).json({ error: message });
});

const port = Number(process.env.PORT ?? 8787);
const host = process.env.HOST ?? '127.0.0.1';
app.listen(port, host, () => console.log(`RelayBox API running at http://${host}:${port}`));
