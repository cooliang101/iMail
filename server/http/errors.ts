import type { ErrorRequestHandler } from 'express';
import { z } from 'zod';

export const errorHandler: ErrorRequestHandler = (error: unknown, _req, res, _next) => {
  if (error instanceof z.ZodError) {
    res.status(400).json({ error: error.issues.map((issue) => issue.message).join('；') });
    return;
  }
  const message = error instanceof Error ? error.message : '服务发生未知错误';
  console.error(error);
  res.status(500).json({ error: message });
};
