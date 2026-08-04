import express, { type Express } from 'express';
import { existsSync } from 'node:fs';
import path from 'node:path';

const RESERVED_PREFIXES = ['/api', '/gateway', '/mcp'];
const WEB_CONTENT_SECURITY_POLICY = "default-src 'self'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'; object-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self' ws: wss:";

export function resolveWebClientRoot() {
  if (process.env.IMAIL_PACKAGED_SERVICE === 'true') return undefined;
  const configured = process.env.IMAIL_WEB_DIST?.trim();
  if (!configured && process.env.NODE_ENV !== 'production') return undefined;
  const root = path.resolve(configured || 'dist');
  return existsSync(path.join(root, 'index.html')) ? root : undefined;
}

export function installWebClient(app: Express, root: string) {
  app.use(express.static(root, {
    index: false,
    setHeaders(response, file) {
      if (file.endsWith('index.html')) {
        response.setHeader('Cache-Control', 'no-cache');
        response.setHeader('Content-Security-Policy', WEB_CONTENT_SECURITY_POLICY);
      }
      else if (file.includes(`${path.sep}assets${path.sep}`)) response.setHeader('Cache-Control', 'public, max-age=31536000, immutable');
    },
  }));
  app.use((request, response, next) => {
    if (request.method !== 'GET' || !request.accepts('html') || RESERVED_PREFIXES.some((prefix) => request.path === prefix || request.path.startsWith(`${prefix}/`))) {
      next(); return;
    }
    response.setHeader('Cache-Control', 'no-cache');
    response.setHeader('Content-Security-Policy', WEB_CONTENT_SECURITY_POLICY);
    response.sendFile(path.join(root, 'index.html'));
  });
}
