import { Router } from 'express';
import { externalAccessSettingsUpdateSchema, readExternalAccessSettings, updateExternalAccessSettings } from '../external-access.js';
import { asyncRoute } from '../http/async-route.js';

export const externalAccessRouter = Router();

externalAccessRouter.get('/external-access', asyncRoute(async (_req, res) => {
  res.json({ settings: await readExternalAccessSettings() });
}));

externalAccessRouter.patch('/external-access', asyncRoute(async (req, res) => {
  const changes = externalAccessSettingsUpdateSchema.parse(req.body);
  res.json({ settings: await updateExternalAccessSettings(changes) });
}));
