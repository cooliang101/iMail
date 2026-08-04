import type { RequestHandler } from 'express';

const developmentClientOrigins = ['http://localhost:5173', 'http://127.0.0.1:5173'];

function hostname(value: string | undefined) {
  if (!value) return '';
  try { return new URL(`http://${value}`).hostname.replace(/^\[|\]$/g, '').toLowerCase(); }
  catch { return ''; }
}

function allowedHosts() {
  return new Set((process.env.IMAIL_ALLOWED_HOSTS || 'localhost,127.0.0.1,::1')
    .split(',').map((value) => value.trim().replace(/^\[|\]$/g, '').toLowerCase()).filter(Boolean));
}

function loopbackHostname(value: string) {
  const normalized = value.replace(/^\[|\]$/g, '').toLowerCase();
  return normalized === 'localhost' || normalized === '::1' || /^127(?:\.\d{1,3}){3}$/.test(normalized);
}

function normalizeOrigin(value: string) {
  const url = new URL(value);
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash || (url.pathname && url.pathname !== '/')) {
    throw new Error(`CORS_ORIGIN 包含无效来源：${value}`);
  }
  if (url.protocol === 'http:' && !loopbackHostname(url.hostname)) {
    throw new Error(`CORS_ORIGIN 的非回环来源必须使用 HTTPS：${value}`);
  }
  return url.origin;
}

export function configuredCorsOrigins(includeDevelopment = process.env.NODE_ENV !== 'production') {
  const configured = (process.env.CORS_ORIGIN || '').split(',').map((value) => value.trim()).filter(Boolean).map(normalizeOrigin);
  return [...new Set([...(includeDevelopment ? developmentClientOrigins : []), ...configured])];
}

export function requestHostAllowed(host: string | undefined) {
  return process.env.NODE_ENV !== 'production' || allowedHosts().has(hostname(host));
}

export function requestOriginAllowed(origin: string | undefined, host: string | undefined) {
  if (!origin) return true;
  let parsed: URL;
  try {
    parsed = new URL(normalizeOrigin(origin));
  } catch { return false; }
  let sameAuthority = false;
  try { sameAuthority = new URL(`${parsed.protocol}//${host || ''}`).host.toLowerCase() === parsed.host.toLowerCase(); }
  catch { /* Invalid Host is rejected separately by the production host boundary. */ }
  if (sameAuthority) {
    return process.env.NODE_ENV !== 'production' || parsed.protocol === 'https:' || loopbackHostname(parsed.hostname);
  }
  return configuredCorsOrigins(false).includes(parsed.origin);
}

export const productionSecurity: RequestHandler = (request, response, next) => {
  response.setHeader('X-Content-Type-Options', 'nosniff');
  response.setHeader('X-Frame-Options', 'DENY');
  response.setHeader('Referrer-Policy', 'same-origin');
  response.setHeader('Permissions-Policy', 'camera=(), microphone=(), geolocation=()');
  if (request.secure) response.setHeader('Strict-Transport-Security', 'max-age=31536000');

  if (!requestHostAllowed(request.header('host'))) {
    response.status(421).json({ error: '请求主机名不在服务允许列表中' }); return;
  }
  next();
};
