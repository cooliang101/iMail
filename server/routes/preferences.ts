import { Router } from 'express';
import { asyncRoute } from '../http/async-route.js';
import { appPreferencesUpdateSchema, readAppPreferences, updateAppPreferences } from '../preferences.js';

export const preferencesRouter = Router();

preferencesRouter.get('/preferences', asyncRoute(async (_req, res) => {
  res.json({ preferences: await readAppPreferences() });
}));

preferencesRouter.patch('/preferences', asyncRoute(async (req, res) => {
  const changes = appPreferencesUpdateSchema.parse(req.body);
  res.json({ preferences: await updateAppPreferences(changes) });
}));
