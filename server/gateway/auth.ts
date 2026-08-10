import type { Request } from 'express';
import { asyncRoute } from '../http/async-route.js';
import { authenticateToken } from '../tokens.js';
import type { TokenScope } from '../types.js';
import { GatewayError } from './errors.js';
import { enterUserContext } from '../auth/context.js';
import { readExternalAccessSettings } from '../external-access.js';

function bearer(req: Request) {
  return req.headers.authorization?.replace(/^Bearer\s+/i, '');
}

export function requireGatewayToken(scope: TokenScope) {
  return asyncRoute(async (req, res, next) => {
    const token = await authenticateToken(bearer(req), scope);
    if (!token) throw new GatewayError(401, 'UNAUTHORIZED', 'Token 无效、已过期或缺少权限');
    if (!token.ownerId || token.ownerId === '__legacy__') throw new GatewayError(401, 'UNAUTHORIZED', 'Token 缺少应用账号归属，请登录后重新创建');
    enterUserContext(token.ownerId);
    if (!(await readExternalAccessSettings()).gatewayEnabled) throw new GatewayError(403, 'GATEWAY_DISABLED', '本地网关尚未在 iMail 界面中启用');
    res.locals.devToken = token;
    next();
  });
}
