import type { AuthInfo } from '@modelcontextprotocol/server';
import { createMcpHandler } from '@modelcontextprotocol/server';
import { once } from 'node:events';
import { Router, type Request, type Response } from 'express';
import { createMailMcpServer } from './server.js';
import { authenticateToken } from '../tokens.js';
import { enterUserContext } from '../auth/context.js';
import { recordSecurityEvent } from '../auth/http.js';

const handler = createMcpHandler(() => createMailMcpServer());
const AUDITED_MCP_TOOLS = new Set([
  'settings_update', 'theme_custom_update',
  'account_add_with_code', 'account_start_oauth', 'account_reconnect_oauth', 'account_update',
  'account_update_authorization_code', 'account_proxy_update', 'account_remove',
  'sync_policy_update',
]);

function bearer(request: Request) {
  const match = request.headers.authorization?.match(/^Bearer\s+(.+)$/i);
  return match?.[1]?.trim();
}

function allowedHostname(hostname: string) {
  const allowed = (process.env.MCP_ALLOWED_HOSTS ?? 'localhost,127.0.0.1,::1').split(',').map((item) => item.trim().toLowerCase()).filter(Boolean);
  const normalized = hostname.toLowerCase().replace(/^\[|\]$/g, '');
  return allowed.some((item) => item.replace(/^\[|\]$/g, '') === normalized);
}

export const mcpRouter = Router();

async function serveMcp(request: Request, response: Response, authInfo: AuthInfo) {
  const controller = new AbortController();
  request.once('aborted', () => controller.abort());
  const headers = new Headers();
  for (const [name, value] of Object.entries(request.headers)) {
    if (Array.isArray(value)) value.forEach((item) => headers.append(name, item));
    else if (value !== undefined) headers.set(name, value);
  }
  const url = `${request.protocol}://${request.get('host')}${request.originalUrl}`;
  const webRequest = new globalThis.Request(url, {
    method: request.method, headers, signal: controller.signal,
    ...(['GET', 'HEAD'].includes(request.method) ? {} : { body: JSON.stringify(request.body ?? {}) }),
  });
  const webResponse = await handler.fetch(webRequest, { authInfo, parsedBody: request.body });
  response.status(webResponse.status);
  webResponse.headers.forEach((value, name) => response.setHeader(name, value));
  if (!webResponse.body || request.method === 'HEAD') { response.end(); return; }
  const reader = webResponse.body.getReader();
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (!response.write(Buffer.from(value))) await once(response, 'drain');
    }
    response.end();
  } finally { reader.releaseLock(); }
}

mcpRouter.all('/mcp', async (req, res) => {
  if (!allowedHostname(req.hostname)) { res.status(403).json({ error: 'MCP Host 不在允许列表' }); return; }
  const origin = req.headers.origin;
  if (origin) {
    try { if (!allowedHostname(new URL(origin).hostname)) { res.status(403).json({ error: 'MCP Origin 不在允许列表' }); return; } }
    catch { res.status(403).json({ error: 'MCP Origin 无效' }); return; }
  }
  const raw = bearer(req);
  const token = await authenticateToken(raw, 'mcp:full');
  if (!token) {
    res.setHeader('WWW-Authenticate', 'Bearer realm="iMail MCP", scope="mcp:full"');
    res.status(401).json({ error: 'MCP 授权码无效、已过期或已撤销' });
    return;
  }
  if (!token.ownerId || token.ownerId === '__legacy__') { res.status(401).json({ error: 'MCP 授权码缺少应用账号归属，请登录后重新创建' }); return; }
  enterUserContext(token.ownerId);
  const toolName = req.body?.method === 'tools/call' && typeof req.body?.params?.name === 'string'
    ? req.body.params.name
    : undefined;
  if (toolName && AUDITED_MCP_TOOLS.has(toolName)) {
    recordSecurityEvent('mcp.management-tool-called', req.ip || 'unknown', token.ownerId, {
      tool: toolName,
      authorizationCodeId: token.id,
    });
  }
  const auth: AuthInfo = {
    token: raw!, clientId: token.id, scopes: token.scopes,
    expiresAt: Math.floor(new Date(token.expiresAt).getTime() / 1000), extra: { authorizationCodeId: token.id },
  };
  await serveMcp(req, res, auth);
});
