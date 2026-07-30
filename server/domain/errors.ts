export class AppError extends Error {
  constructor(
    public readonly code: string,
    public readonly status: number,
    message: string,
  ) { super(message); }
}

export function notFound(code: string, message: string) { return new AppError(code, 404, message); }
export function conflict(code: string, message: string) { return new AppError(code, 409, message); }
export function invalid(code: string, message: string) { return new AppError(code, 400, message); }
