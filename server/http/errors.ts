import type { ErrorRequestHandler } from 'express';
import { z } from 'zod';
import { AppError } from '../domain/errors.js';

export const errorHandler: ErrorRequestHandler = (error: unknown, _req, res, _next) => {
  if (error instanceof z.ZodError) {
    res.status(400).json({ error: error.issues.map((issue) => issue.message).join('；') });
    return;
  }
  if (error instanceof AppError) {
    res.status(error.status).json({ error: error.message });
    return;
  }
  const detail = (error instanceof Error ? error.stack || error.message : String(error))
    .replace(/Bearer\s+[^\s,;]+/gi, 'Bearer [redacted]')
    .replace(/(access[_-]?token|refresh[_-]?token|password|authorization)(\s*[:=]\s*)[^\s,;]+/gi, '$1$2[redacted]')
    .slice(0, 2_000);
  console.error('[http] unhandled request error', detail);
  res.status(500).json({ error: '服务暂时无法完成请求' });
};
