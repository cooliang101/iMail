import type { Request } from 'express';
import { asyncRoute } from '../http/async-route.js';
import { authenticateToken } from '../tokens.js';
import type { TokenScope } from '../types.js';
import { GatewayError } from './errors.js';

function bearer(req: Request) {
  return req.headers.authorization?.replace(/^Bearer\s+/i, '');
}

export function requireGatewayToken(scope: TokenScope) {
  return asyncRoute(async (req, res, next) => {
    const token = await authenticateToken(bearer(req), scope);
    if (!token) throw new GatewayError(401, 'UNAUTHORIZED', 'Token 无效、已过期或缺少权限');
    res.locals.devToken = token;
    next();
  });
}
