import { Router } from 'express';
import { z } from 'zod';
import { currentUserId } from '../auth/context.js';
import { listSecurityEvents } from '../auth/http.js';

export const securityRouter = Router();

securityRouter.get('/security/audit-events', (req, res) => {
  const { limit } = z.object({ limit: z.coerce.number().int().min(1).max(500).default(100) }).parse(req.query);
  const userId = currentUserId();
  if (!userId) { res.status(401).json({ error: '请先登录' }); return; }
  res.json({ events: listSecurityEvents(userId, limit) });
});
