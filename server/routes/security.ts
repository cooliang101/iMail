import { Router } from 'express';
import { z } from 'zod';
import { currentUserId } from '../auth/context.js';
import { listSecurityEvents, reauthenticateSensitiveAction, recordRequestSecurityEvent } from '../auth/http.js';
import { asyncRoute } from '../http/async-route.js';
import {
  clearCurrentUserMailData,
  consumeMailAuthorizationExport,
  prepareMailAuthorizationExport,
} from '../domain/privacy.js';

export const securityRouter = Router();

const currentPasswordSchema = z.string().min(1).max(256);
const prepareAuthorizationExportSchema = z.object({
  currentPassword: currentPasswordSchema,
  exportPassword: z.string().min(12, '导出文件密码至少需要 12 个字符').max(256),
});
const clearUserDataSchema = z.object({
  currentPassword: currentPasswordSchema,
  confirmation: z.literal('清除我的邮箱数据'),
});

securityRouter.get('/security/audit-events', (req, res) => {
  const { limit } = z.object({ limit: z.coerce.number().int().min(1).max(500).default(100) }).parse(req.query);
  const userId = currentUserId();
  if (!userId) { res.status(401).json({ error: '请先登录' }); return; }
  res.json({ events: listSecurityEvents(userId, limit) });
});

securityRouter.post('/security/mail-authorization-exports', asyncRoute(async (req, res) => {
  const input = prepareAuthorizationExportSchema.parse(req.body);
  if (!await reauthenticateSensitiveAction(req, res, input.currentPassword, 'mail-authorization-export')) return;
  const userId = currentUserId();
  if (!userId) { res.status(401).json({ error: '请先登录' }); return; }
  const prepared = await prepareMailAuthorizationExport(userId, input.exportPassword);
  recordRequestSecurityEvent(req, res, 'privacy.mail-authorization-export.prepared', { accountCount: String(prepared.accountCount) });
  res.setHeader('Cache-Control', 'private, no-store, max-age=0');
  res.json({
    downloadPath: `/api/security/mail-authorization-exports/${prepared.id}`,
    filename: prepared.filename,
    accountCount: prepared.accountCount,
    expiresAt: prepared.expiresAt,
  });
}));

securityRouter.get('/security/mail-authorization-exports/:id', (req, res) => {
  const userId = currentUserId();
  if (!userId) { res.status(401).json({ error: '请先登录' }); return; }
  const pending = consumeMailAuthorizationExport(String(req.params.id), userId);
  if (!pending) { res.status(404).json({ error: '导出文件不存在、已过期或已经下载' }); return; }

  let erased = false;
  const erase = () => {
    if (erased) return;
    erased = true;
    pending.body.fill(0);
  };
  res.once('finish', erase);
  res.once('close', erase);
  res.status(200);
  res.setHeader('Content-Type', 'application/vnd.imail.mail-authorization-export+json; charset=utf-8');
  res.setHeader('Content-Disposition', `attachment; filename="${pending.filename}"`);
  res.setHeader('Cache-Control', 'private, no-store, max-age=0');
  res.setHeader('Pragma', 'no-cache');
  res.setHeader('Expires', '0');
  res.setHeader('Content-Length', String(pending.body.length));
  recordRequestSecurityEvent(req, res, 'privacy.mail-authorization-export.downloaded', { accountCount: String(pending.accountCount) });
  res.end(pending.body);
});

securityRouter.post('/security/clear-user-data', asyncRoute(async (req, res) => {
  const input = clearUserDataSchema.parse(req.body);
  if (!await reauthenticateSensitiveAction(req, res, input.currentPassword, 'clear-user-data')) return;
  const userId = currentUserId();
  if (!userId) { res.status(401).json({ error: '请先登录' }); return; }
  const cleared = await clearCurrentUserMailData(userId);
  recordRequestSecurityEvent(req, res, 'privacy.user-data-cleared', { accountCount: String(cleared.accountCount) });
  res.status(204).end();
}));
