import { Router } from 'express';
import { z } from 'zod';
import { createDraft, deleteDraft, listDrafts, saveDraft } from '../domain/drafts.js';
import { asyncRoute } from '../http/async-route.js';
import { draftSchema } from '../http/schemas.js';

export const draftsRouter = Router();

draftsRouter.get('/drafts', asyncRoute(async (_req, res) => {
  res.json({ drafts: (await listDrafts()).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)) });
}));

draftsRouter.post('/drafts', asyncRoute(async (req, res) => {
  const requestedId = z.string().uuid().optional().parse(req.get('X-Draft-Id'));
  const draft = await createDraft(draftSchema.parse(req.body), requestedId);
  res.status(201).json({ draft });
}));

draftsRouter.put('/drafts/:id', asyncRoute(async (req, res) => {
  const draft = await saveDraft(draftSchema.parse(req.body), String(req.params.id));
  res.json({ draft });
}));

draftsRouter.delete('/drafts/:id', asyncRoute(async (req, res) => {
  await deleteDraft(String(req.params.id));
  res.status(204).end();
}));
