import type { NextFunction, Request, Response } from 'express';
import { Router } from 'express';
import { z } from 'zod';
import { asyncRoute } from '../http/async-route.js';
import { enterUserContext } from './context.js';
import { AuthStore, type AppUser } from './store.js';

export const SESSION_COOKIE = 'imail_session';
let authStore: AuthStore | undefined;
function store() { return authStore ??= new AuthStore(); }
const loginAttempts = new Map<string, { count: number; resetAt: number }>();

function cookies(req: Request) {
  return Object.fromEntries((req.headers.cookie ?? '').split(';').map((item) => item.trim().split('=').map(decodeURIComponent)).filter((item) => item.length === 2));
}
function session(req: Request) { return cookies(req)[SESSION_COOKIE]; }
function publicUser(user: AppUser) { return { id: user.id, login: user.login, displayName: user.displayName }; }
function setSessionCookie(req: Request, res: Response, value: string) {
  const secure = req.secure || process.env.NODE_ENV === 'production';
  res.setHeader('Set-Cookie', `${SESSION_COOKIE}=${encodeURIComponent(value)}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000${secure ? '; Secure' : ''}`);
}
function clearSessionCookie(req: Request, res: Response) {
  const secure = req.secure || process.env.NODE_ENV === 'production';
  res.setHeader('Set-Cookie', `${SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure ? '; Secure' : ''}`);
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
  const attemptKey = `${req.ip}:${input.login.toLowerCase()}`;
  const previous = loginAttempts.get(attemptKey);
  if (previous && previous.resetAt > Date.now() && previous.count >= 10) { res.status(429).json({ error: '登录尝试过多，请稍后再试' }); return; }
  const user = await store().authenticate(input.login, input.password);
  if (!user) {
    loginAttempts.set(attemptKey, previous && previous.resetAt > Date.now() ? { ...previous, count: previous.count + 1 } : { count: 1, resetAt: Date.now() + 15 * 60_000 });
    res.status(401).json({ error: '登录名或密码错误' }); return;
  }
  loginAttempts.delete(attemptKey);
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

export function closeAuthStore() { authStore?.close(); authStore = undefined; }
