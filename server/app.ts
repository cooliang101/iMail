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
import { authRouter, requireAppSession } from './auth/http.js';
import { createServiceInfoRouter } from './routes/service-info.js';
import { installWebClient, resolveWebClientRoot } from './web-client.js';
import { configuredCorsOrigins, productionSecurity } from './http/production-security.js';
import { securityRouter } from './routes/security.js';

export function createApp(options: { webRoot?: string | false } = {}) {
  const app = express();
  const webRoot = options.webRoot === false ? undefined : options.webRoot ?? resolveWebClientRoot();
  if (process.env.IMAIL_TRUST_PROXY === 'true') app.set('trust proxy', 1);

  app.use(productionSecurity);
  app.use(cors({ origin: configuredCorsOrigins(), credentials: true }));
  app.use(express.json({ limit: '25mb' }));
  app.use('/api', createServiceInfoRouter(Boolean(webRoot)));
  app.use('/api', authRouter);
  app.use('/api', requireAppSession);
  app.use('/api', systemRouter);
  app.use('/api', oauthRouter);
  app.use('/api', accountsRouter);
  app.use('/api', syncRouter);
  app.use('/api', messagesRouter);
  app.use('/api', preferencesRouter);
  app.use('/api', draftsRouter);
  app.use('/api', developerTokensRouter);
  app.use('/api', securityRouter);
  app.use('/gateway', gatewayDocsRouter);
  app.use('/gateway/v1', gatewayRouter);
  app.use(mcpRouter);
  if (webRoot) installWebClient(app, webRoot);
  app.use(errorHandler);

  return app;
}

export const app = createApp();
