import type { ErrorRequestHandler, RequestHandler, Response } from 'express';
import { z } from 'zod';
import { AppError } from '../domain/errors.js';

export class GatewayError extends Error {
  constructor(public readonly status: number, public readonly code: string, message: string, public readonly details?: unknown) {
    super(message);
  }
}

export function sendGatewayError(res: Response, status: number, code: string, message: string, details?: unknown) {
  res.status(status).json({
    error: {
      code,
      message,
      requestId: res.locals.requestId,
      ...(details === undefined ? {} : { details }),
    },
  });
}

export const gatewayRequestContext: RequestHandler = (req, res, next) => {
  const supplied = typeof req.headers['x-request-id'] === 'string' ? req.headers['x-request-id'].trim() : '';
  const requestId = supplied && /^[A-Za-z0-9._:-]{1,128}$/.test(supplied) ? supplied : crypto.randomUUID();
  res.locals.requestId = requestId;
  res.setHeader('X-Request-Id', requestId);
  res.setHeader('Cache-Control', 'no-store');
  next();
};

export const gatewayErrorHandler: ErrorRequestHandler = (error: unknown, _req, res, _next) => {
  if (error instanceof AppError) {
    sendGatewayError(res, error.status, error.code, error.message);
    return;
  }
  if (error instanceof GatewayError) {
    sendGatewayError(res, error.status, error.code, error.message, error.details);
    return;
  }
  if (error instanceof z.ZodError) {
    sendGatewayError(res, 400, 'INVALID_REQUEST', '请求参数无效', error.issues.map((issue) => ({ path: issue.path.join('.'), message: issue.message })));
    return;
  }
  console.error(error);
  sendGatewayError(res, 500, 'INTERNAL_ERROR', '网关处理请求时发生错误');
};

export const gatewayNotFound: RequestHandler = (_req, res) => {
  sendGatewayError(res, 404, 'ENDPOINT_NOT_FOUND', '开发者网关接口不存在');
};
