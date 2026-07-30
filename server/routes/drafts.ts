import crypto from 'node:crypto';
import { Router } from 'express';
import { asyncRoute } from '../http/async-route.js';
import { draftSchema } from '../http/schemas.js';
import { readStore, updateStore } from '../store.js';

export const draftsRouter = Router();

draftsRouter.get('/drafts', asyncRoute(async (_req, res) => {
  const data = await readStore();
  res.json({ drafts: (data.drafts ?? []).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)) });
}));

draftsRouter.post('/drafts', asyncRoute(async (req, res) => {
  const input = draftSchema.parse(req.body); const now = new Date().toISOString();
  const draft = { id: crypto.randomUUID(), ...input, createdAt: now, updatedAt: now };
  await updateStore((data) => {
    if (!data.accounts.some((item) => item.id === input.accountId)) throw new Error('发件邮箱不存在');
    (data.drafts ??= []).push(draft);
  });
  res.status(201).json({ draft });
}));

draftsRouter.put('/drafts/:id', asyncRoute(async (req, res) => {
  const input = draftSchema.parse(req.body); let draft;
  const existing = await readStore();
  if (!existing.accounts.some((item) => item.id === input.accountId)) { res.status(404).json({ error: '发件邮箱不存在' }); return; }
  if (!(existing.drafts ?? []).some((item) => item.id === req.params.id)) { res.status(404).json({ error: '草稿不存在' }); return; }
  await updateStore((data) => {
    if (!data.accounts.some((item) => item.id === input.accountId)) throw new Error('发件邮箱不存在');
    draft = (data.drafts ?? []).find((item) => item.id === req.params.id);
    if (!draft) throw new Error('草稿不存在');
    Object.assign(draft, input, { updatedAt: new Date().toISOString() });
  });
  res.json({ draft });
}));

draftsRouter.delete('/drafts/:id', asyncRoute(async (req, res) => {
  await updateStore((data) => { data.drafts = (data.drafts ?? []).filter((item) => item.id !== req.params.id); });
  res.status(204).end();
}));
