import type { NextFunction, Request, Response } from 'express';
import { Router } from 'express';
import { z } from 'zod';
import { asyncRoute } from '../http/async-route.js';
import { enterUserContext } from './context.js';
import { AuthStore, type AppUser } from './store.js';

export const SESSION_COOKIE = 'imail_session';
let authStore: AuthStore | undefined;
function store() { return authStore ??= new AuthStore(); }
function consumeAttempt(key: string, maximum: number, windowMs: number) {
  return store().consumeAttempt(key, maximum, windowMs);
}

function rejectLimited(res: Response, retryAfter: number) {
  res.setHeader('Retry-After', String(retryAfter));
  res.status(429).json({ error: '尝试过多，请稍后再试' });
}

const SENSITIVE_ACTION_USER_ATTEMPTS = 5;
const SENSITIVE_ACTION_SOURCE_ATTEMPTS = 20;
const SENSITIVE_ACTION_WINDOW_MS = 15 * 60_000;

function cookies(req: Request) {
  const result: Record<string, string> = {};
  for (const item of (req.headers.cookie ?? '').split(';')) {
    const separator = item.indexOf('=');
    if (separator < 1) continue;
    try { result[decodeURIComponent(item.slice(0, separator).trim())] = decodeURIComponent(item.slice(separator + 1).trim()); }
    catch { /* Ignore malformed cookies instead of failing the whole request. */ }
  }
  return result;
}
function session(req: Request) { return cookies(req)[SESSION_COOKIE]; }
function publicUser(user: AppUser) { return { id: user.id, login: user.login, displayName: user.displayName }; }
function registrationOpen() {
  if (store().setupRequired()) return true;
  const mode = process.env.IMAIL_REGISTRATION_MODE?.trim().toLowerCase();
  return mode === 'open' || (!mode && process.env.NODE_ENV !== 'production');
}
function setSessionCookie(req: Request, res: Response, value: string) {
  const crossOrigin = requestIsCrossOrigin(req);
  const secure = crossOrigin || req.secure;
  res.setHeader('Set-Cookie', `${SESSION_COOKIE}=${encodeURIComponent(value)}; Path=/; HttpOnly; SameSite=${crossOrigin ? 'None' : 'Lax'}; Max-Age=2592000${secure ? '; Secure' : ''}`);
}
function clearSessionCookie(req: Request, res: Response) {
  const crossOrigin = requestIsCrossOrigin(req);
  const secure = crossOrigin || req.secure;
  res.setHeader('Set-Cookie', `${SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=${crossOrigin ? 'None' : 'Lax'}; Max-Age=0${secure ? '; Secure' : ''}`);
}

function requestIsCrossOrigin(req: Request) {
  const origin = req.header('origin');
  if (!origin) return false;
  try { return new URL(origin).host !== req.header('host'); }
  catch { return false; }
}

const credentials = z.object({
  login: z.string().trim().min(3, '登录名至少 3 个字符').max(80).regex(/^[\p{L}\p{N}_.@-]+$/u, '登录名包含不支持的字符'),
  password: z.string().min(8, '密码至少 8 个字符').max(256),
});

export const authRouter = Router();
authRouter.get('/auth/status', (req, res) => {
  const user = store().userForSession(session(req));
  res.json({ setupRequired: store().setupRequired(), registrationOpen: registrationOpen(), user: user ? publicUser(user) : null });
});
authRouter.get('/auth/session', (req, res) => {
  const user = store().userForSession(session(req));
  if (!user) { res.status(401).json({ error: '请先登录' }); return; }
  res.json({ user: publicUser(user) });
});
authRouter.post('/auth/register', asyncRoute(async (req, res) => {
  if (!registrationOpen()) {
    store().recordSecurityEvent('registration.denied', req.ip || 'unknown');
    res.status(403).json({ error: '此服务已关闭新用户注册' }); return;
  }
  const registrationLimit = consumeAttempt(`register:${req.ip}`, 5, 60 * 60_000);
  if (!registrationLimit.allowed) { rejectLimited(res, registrationLimit.retryAfter); return; }
  const input = credentials.extend({ displayName: z.string().trim().min(1, '请填写显示名称').max(80) }).parse(req.body);
  try {
    const user = await store().createUser(input);
    store().recordSecurityEvent('registration.succeeded', req.ip || 'unknown', user.id);
    setSessionCookie(req, res, store().createSession(user.id));
    res.status(201).json({ user: publicUser(user) });
  } catch (error) {
    if (String(error).includes('UNIQUE constraint failed')) { res.status(409).json({ error: '这个登录名已存在' }); return; }
    throw error;
  }
}));
authRouter.post('/auth/login', asyncRoute(async (req, res) => {
  const input = credentials.parse(req.body);
  const ipLimit = consumeAttempt(`login-ip:${req.ip}`, 30, 15 * 60_000);
  if (!ipLimit.allowed) { rejectLimited(res, ipLimit.retryAfter); return; }
  const accountKey = `login-account:${input.login.toLowerCase()}`;
  const accountLimit = consumeAttempt(accountKey, 10, 15 * 60_000);
  if (!accountLimit.allowed) { rejectLimited(res, accountLimit.retryAfter); return; }
  const user = await store().authenticate(input.login, input.password);
  if (!user) {
    store().recordSecurityEvent('login.failed', req.ip || 'unknown');
    res.status(401).json({ error: '登录名或密码错误' }); return;
  }
  store().clearAttempt(accountKey);
  store().recordSecurityEvent('login.succeeded', req.ip || 'unknown', user.id);
  setSessionCookie(req, res, store().createSession(user.id));
  res.json({ user: publicUser(user) });
}));
authRouter.post('/auth/logout', (req, res) => {
  const user = store().userForSession(session(req));
  if (user) store().recordSecurityEvent('logout', req.ip || 'unknown', user.id);
  store().deleteSession(session(req)); clearSessionCookie(req, res); res.status(204).end();
});

export function requireAppSession(req: Request, res: Response, next: NextFunction) {
  if (req.path === '/health' || /^\/oauth\/(google|microsoft|yahoo)\/callback$/.test(req.path)) { next(); return; }
  const user = store().userForSession(session(req));
  if (!user) { res.status(401).json({ error: '登录已过期，请重新登录' }); return; }
  res.locals.appUser = publicUser(user);
  enterUserContext(user.id);
  next();
}

export function recordRequestSecurityEvent(req: Request, res: Response, eventType: string, detail: Record<string, string> = {}) {
  const user = res.locals.appUser as { id?: string } | undefined;
  recordSecurityEvent(eventType, req.ip || 'unknown', user?.id, detail);
}

export function recordSecurityEvent(eventType: string, actor: string, userId?: string, detail: Record<string, string> = {}) {
  store().recordSecurityEvent(eventType, actor, userId, detail);
}

export async function reauthenticateSensitiveAction(req: Request, res: Response, password: string, action: string) {
  const user = res.locals.appUser as { id?: string } | undefined;
  if (!user?.id) { res.status(401).json({ error: '请先登录' }); return false; }

  const actor = req.ip || 'unknown';
  const sourceKey = `sensitive-action:${action}:source:${actor}`;
  const userKey = `sensitive-action:${action}:user:${user.id}`;
  const sourceLimit = consumeAttempt(sourceKey, SENSITIVE_ACTION_SOURCE_ATTEMPTS, SENSITIVE_ACTION_WINDOW_MS);
  const userLimit = consumeAttempt(userKey, SENSITIVE_ACTION_USER_ATTEMPTS, SENSITIVE_ACTION_WINDOW_MS);
  if (!sourceLimit.allowed || !userLimit.allowed) {
    store().recordSecurityEvent('sensitive-action.reauthentication-rate-limited', actor, user.id, { action });
    rejectLimited(res, Math.max(sourceLimit.retryAfter, userLimit.retryAfter));
    return false;
  }

  if (!await store().verifyUserPassword(user.id, password)) {
    store().recordSecurityEvent('sensitive-action.reauthentication-failed', actor, user.id, { action });
    res.status(403).json({ error: '当前 iMail 密码不正确' });
    return false;
  }

  store().clearAttempt(sourceKey);
  store().clearAttempt(userKey);
  return true;
}

export function listSecurityEvents(userId: string, limit?: number) {
  return store().listSecurityEvents(userId, limit);
}

export function closeAuthStore() { authStore?.close(); authStore = undefined; }
