import cors from 'cors';
import express from 'express';
import { errorHandler } from './http/errors.js';
import { accountsRouter } from './routes/accounts.js';
import { developerTokensRouter } from './routes/developer-tokens.js';
import { draftsRouter } from './routes/drafts.js';
import { gatewayRouter } from './routes/gateway.js';
import { gatewayDocsRouter } from './routes/gateway-docs.js';
import { messagesRouter } from './routes/messages.js';
import { oauthRouter } from './routes/oauth-routes.js';
import { systemRouter } from './routes/system.js';

export function createApp() {
  const app = express();
  const origins = (process.env.CORS_ORIGIN ?? 'http://localhost:5173').split(',').map((item) => item.trim());

  app.use(cors({ origin: origins }));
  app.use(express.json({ limit: '2mb' }));
  app.use('/api', systemRouter);
  app.use('/api', oauthRouter);
  app.use('/api', accountsRouter);
  app.use('/api', messagesRouter);
  app.use('/api', draftsRouter);
  app.use('/api', developerTokensRouter);
  app.use('/api', gatewayDocsRouter);
  app.use('/api/dev/v1', gatewayRouter);
  app.use(errorHandler);

  return app;
}

export const app = createApp();
