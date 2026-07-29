import type { NextFunction, Request, RequestHandler, Response } from 'express';

type AsyncHandler = (req: Request, res: Response, next: NextFunction) => Promise<unknown>;

export function asyncRoute(handler: AsyncHandler): RequestHandler {
  return (req, res, next) => void handler(req, res, next).catch(next);
}
