import { Router } from 'express';
import { asyncRoute } from '../http/async-route.js';
import { publicDeveloperToken } from '../http/presenters.js';
import { tokenSchema } from '../http/schemas.js';
import { readStore, updateStore } from '../store.js';
import { issueToken } from '../tokens.js';
import type { TokenScope } from '../types.js';
import { invalid } from '../domain/errors.js';

export const developerTokensRouter = Router();

developerTokensRouter.get('/developer-tokens', asyncRoute(async (_req, res) => {
  const data = await readStore();
  res.json({ tokens: data.tokens.map((token) => publicDeveloperToken(token, data.accounts)).sort((a, b) => b.createdAt.localeCompare(a.createdAt)) });
}));

developerTokensRouter.post('/developer-tokens', asyncRoute(async (req, res) => {
  const input = tokenSchema.parse(req.body);
  const data = await readStore();
  const requestedMailboxes = new Set(input.mailboxes.map((email) => email.toLowerCase()));
  const accounts = data.accounts.filter((account) => requestedMailboxes.has(account.email.toLowerCase()));
  if (accounts.length !== requestedMailboxes.size) throw invalid('TOKEN_MAILBOX_NOT_FOUND', '包含不存在的邮箱账户');
  const scopes: TokenScope[] = input.scopes.includes('mcp:full') ? ['mcp:full'] : input.scopes as TokenScope[];
  const accountIds = scopes.includes('mcp:full') ? data.accounts.map((account) => account.id) : accounts.map((account) => account.id);
  const result = await issueToken({ name: input.name, scopes, accountIds, ttlSeconds: input.ttlSeconds });
  res.status(201).json({ token: result.raw, detail: publicDeveloperToken(result.token, data.accounts) });
}));

developerTokensRouter.delete('/developer-tokens/:id', asyncRoute(async (req, res) => {
  await updateStore((data) => { data.tokens = data.tokens.filter((item) => item.id !== req.params.id); });
  res.status(204).end();
}));
