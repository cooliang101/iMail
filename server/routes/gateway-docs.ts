import { Router } from 'express';
import { gatewayDocsPage } from '../gateway/docs-page.js';
import { gatewayOpenApi } from '../gateway/openapi.js';

export const gatewayDocsRouter = Router();

gatewayDocsRouter.get('/openapi.json', (_req, res) => {
  res.setHeader('Cache-Control', 'no-store');
  res.json(gatewayOpenApi);
});

gatewayDocsRouter.get(['/docs', '/docs/'], (_req, res) => {
  res.setHeader('Content-Security-Policy', "default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self' ws: wss:; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'");
  res.type('html').send(gatewayDocsPage());
});
