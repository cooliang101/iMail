import { timingSafeEqual } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import express, { type Express, type NextFunction, type Request, type Response } from 'express';

export type DesktopRuntimeConfig = {
  enabled: boolean;
  startupToken?: string;
  webDirectory?: string;
};

export const DESKTOP_CONTENT_SECURITY_POLICY = [
  "default-src 'self'",
  "connect-src 'self'",
  "img-src 'self' data: blob:",
  "style-src 'self' 'unsafe-inline'",
  "script-src 'self'",
  "frame-src 'self'",
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'self'",
].join('; ');

export function desktopRuntimeConfig(environment: NodeJS.ProcessEnv = process.env): DesktopRuntimeConfig {
  return {
    enabled: environment.IMAIL_DESKTOP_MODE === 'true',
    startupToken: environment.IMAIL_DESKTOP_STARTUP_TOKEN?.trim() || undefined,
    webDirectory: environment.IMAIL_WEB_DIR ? path.resolve(environment.IMAIL_WEB_DIR) : undefined,
  };
}

function equalSecret(left: string, right: string) {
  const actual = Buffer.from(left);
  const expected = Buffer.from(right);
  return actual.length === expected.length && timingSafeEqual(actual, expected);
}

export function desktopRuntimeMiddleware(config = desktopRuntimeConfig()) {
  return (req: Request, res: Response, next: NextFunction) => {
    if (!config.enabled) { next(); return; }
    if (!['127.0.0.1', 'localhost', '::1'].includes(req.hostname)) {
      res.status(421).json({ error: '桌面服务只接受本机回环地址' }); return;
    }
    res.setHeader('X-Content-Type-Options', 'nosniff');
    res.setHeader('Referrer-Policy', 'no-referrer');
    res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
    res.setHeader('Content-Security-Policy', DESKTOP_CONTENT_SECURITY_POLICY);
    next();
  };
}

export function desktopStartupHealth(config = desktopRuntimeConfig()) {
  return (req: Request, res: Response, next: NextFunction) => {
    if (!config.enabled) { next(); return; }
    if (!config.startupToken) { res.status(503).json({ error: '桌面启动握手未配置' }); return; }
    const supplied = req.header('x-imail-startup-token') ?? '';
    if (!equalSecret(supplied, config.startupToken)) { res.status(401).json({ error: '桌面启动握手失败' }); return; }
    res.json({ ok: true, service: 'imail-desktop', pid: process.pid });
  };
}

export function installDesktopStaticApp(app: Express, config = desktopRuntimeConfig()) {
  const indexFile = config.webDirectory ? path.join(config.webDirectory, 'index.html') : undefined;
  if (!config.enabled || !config.webDirectory || !indexFile || !existsSync(indexFile)) return;
  const indexHtml = readFileSync(indexFile, 'utf8');
  app.use(express.static(config.webDirectory, {
    index: false,
    maxAge: '1y',
    immutable: true,
    setHeaders(response, file) {
      if (path.basename(file) === 'index.html') response.setHeader('Cache-Control', 'no-store');
    },
  }));
  app.use((req, res, next) => {
    if (req.method !== 'GET' || !req.accepts('html')) { next(); return; }
    if (req.path.startsWith('/api') || req.path.startsWith('/gateway') || req.path.startsWith('/mcp')) { next(); return; }
    res.setHeader('Cache-Control', 'no-store');
    res.type('html').send(indexHtml);
  });
}
