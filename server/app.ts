import cors from 'cors';
import express from 'express';
import { errorHandler } from './http/errors.js';
import { accountsRouter } from './routes/accounts.js';
import { developerTokensRouter } from './routes/developer-tokens.js';
import { draftsRouter } from './routes/drafts.js';
import { gatewayRouter } from './routes/gateway.js';
import { gatewayDocsRouter } from './routes/gateway-docs.js';
import { messagesRouter } from './routes/messages.js';
import { preferencesRouter } from './routes/preferences.js';
import { mcpRouter } from './mcp/http.js';
import { oauthRouter } from './routes/oauth-routes.js';
import { systemRouter } from './routes/system.js';
import { syncRouter } from './routes/sync.js';

export function createApp() {
  const app = express();
  const origins = (process.env.CORS_ORIGIN ?? 'http://localhost:5173').split(',').map((item) => item.trim());

  app.use(cors({ origin: origins }));
  app.use(express.json({ limit: '25mb' }));
  app.use('/api', systemRouter);
  app.use('/api', oauthRouter);
  app.use('/api', accountsRouter);
  app.use('/api', syncRouter);
  app.use('/api', messagesRouter);
  app.use('/api', preferencesRouter);
  app.use('/api', draftsRouter);
  app.use('/api', developerTokensRouter);
  app.use('/gateway', gatewayDocsRouter);
  app.use('/gateway/v1', gatewayRouter);
  app.use(mcpRouter);
  app.use(errorHandler);

  return app;
}

export const app = createApp();
