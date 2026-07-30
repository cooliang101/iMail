import type { NextFunction, Request, Response } from 'express';
import { Router } from 'express';
import { z } from 'zod';
import { asyncRoute } from '../http/async-route.js';
import { enterUserContext } from './context.js';
import { AuthStore, type AppUser } from './store.js';

export const SESSION_COOKIE = 'imail_session';
let authStore: AuthStore | undefined;
function store() { return authStore ??= new AuthStore(); }
const attempts = new Map<string, { count: number; resetAt: number }>();
const MAX_ATTEMPT_KEYS = 10_000;

function consumeAttempt(key: string, maximum: number, windowMs: number) {
  const now = Date.now();
  if (attempts.size >= MAX_ATTEMPT_KEYS) {
    for (const [candidate, value] of attempts) if (value.resetAt <= now) attempts.delete(candidate);
    if (attempts.size >= MAX_ATTEMPT_KEYS) attempts.delete(attempts.keys().next().value as string);
  }
  const previous = attempts.get(key);
  const current = !previous || previous.resetAt <= now ? { count: 0, resetAt: now + windowMs } : previous;
  if (current.count >= maximum) return { allowed: false, retryAfter: Math.max(1, Math.ceil((current.resetAt - now) / 1_000)) };
  current.count += 1;
  attempts.set(key, current);
  return { allowed: true, retryAfter: 0 };
}

function rejectLimited(res: Response, retryAfter: number) {
  res.setHeader('Retry-After', String(retryAfter));
  res.status(429).json({ error: '尝试过多，请稍后再试' });
}

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
function setSessionCookie(req: Request, res: Response, value: string) {
  const desktop = process.env.IMAIL_DESKTOP_MODE === 'true';
  const secure = req.secure || (!desktop && process.env.NODE_ENV === 'production');
  res.setHeader('Set-Cookie', `${SESSION_COOKIE}=${encodeURIComponent(value)}; Path=/; HttpOnly; SameSite=${desktop ? 'Strict' : 'Lax'}; Max-Age=2592000${secure ? '; Secure' : ''}`);
}
function clearSessionCookie(req: Request, res: Response) {
  const desktop = process.env.IMAIL_DESKTOP_MODE === 'true';
  const secure = req.secure || (!desktop && process.env.NODE_ENV === 'production');
  res.setHeader('Set-Cookie', `${SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=${desktop ? 'Strict' : 'Lax'}; Max-Age=0${secure ? '; Secure' : ''}`);
}

const credentials = z.object({
  login: z.string().trim().min(3, '登录名至少 3 个字符').max(80).regex(/^[\p{L}\p{N}_.@-]+$/u, '登录名包含不支持的字符'),
  password: z.string().min(8, '密码至少 8 个字符').max(256),
});

export const authRouter = Router();
authRouter.get('/auth/status', (req, res) => {
  const user = store().userForSession(session(req));
  res.json({ setupRequired: store().setupRequired(), user: user ? publicUser(user) : null });
});
authRouter.get('/auth/session', (req, res) => {
  const user = store().userForSession(session(req));
  if (!user) { res.status(401).json({ error: '请先登录' }); return; }
  res.json({ user: publicUser(user) });
});
authRouter.post('/auth/register', asyncRoute(async (req, res) => {
  const registrationLimit = consumeAttempt(`register:${req.ip}`, 5, 60 * 60_000);
  if (!registrationLimit.allowed) { rejectLimited(res, registrationLimit.retryAfter); return; }
  const input = credentials.extend({ displayName: z.string().trim().min(1, '请填写显示名称').max(80) }).parse(req.body);
  try {
    const user = await store().createUser(input);
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
    res.status(401).json({ error: '登录名或密码错误' }); return;
  }
  attempts.delete(accountKey);
  setSessionCookie(req, res, store().createSession(user.id));
  res.json({ user: publicUser(user) });
}));
authRouter.post('/auth/logout', (req, res) => { store().deleteSession(session(req)); clearSessionCookie(req, res); res.status(204).end(); });

export function requireAppSession(req: Request, res: Response, next: NextFunction) {
  if (req.path === '/health' || /^\/oauth\/(google|microsoft|yahoo)\/callback$/.test(req.path)) { next(); return; }
  const user = store().userForSession(session(req));
  if (!user) { res.status(401).json({ error: '登录已过期，请重新登录' }); return; }
  res.locals.appUser = publicUser(user);
  enterUserContext(user.id);
  next();
}

export function closeAuthStore() { authStore?.close(); authStore = undefined; attempts.clear(); }
