import type { Request } from 'express';
import { asyncRoute } from './async-route.js';
import { authenticateToken } from '../tokens.js';
import type { TokenScope } from '../types.js';

function bearer(req: Request) {
  return req.headers.authorization?.replace(/^Bearer\s+/i, '');
}

export function requireDevToken(scope: TokenScope) {
  return asyncRoute(async (req, res, next) => {
    const token = await authenticateToken(bearer(req), scope);
    if (!token) { res.status(401).json({ error: 'Token 无效、已过期或缺少权限' }); return; }
    res.locals.devToken = token;
    next();
  });
}
