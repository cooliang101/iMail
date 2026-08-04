import type { NextFunction, Request, Response } from 'express';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { configuredCorsOrigins, productionSecurity, requestOriginAllowed } from './production-security.js';

afterEach(() => {
  delete process.env.NODE_ENV;
  delete process.env.IMAIL_ALLOWED_HOSTS;
  delete process.env.CORS_ORIGIN;
});

function responseDouble() {
  const headers = new Map<string, string>();
  const response = {
    setHeader: vi.fn((name: string, value: string) => headers.set(name, value)),
    status: vi.fn(), json: vi.fn(),
  };
  response.status.mockReturnValue(response);
  return { response: response as unknown as Response, headers };
}

describe('production HTTP boundary', () => {
  it('rejects unknown hosts in production and applies baseline headers', () => {
    process.env.NODE_ENV = 'production';
    process.env.IMAIL_ALLOWED_HOSTS = 'mail.example.com';
    const { response, headers } = responseDouble();
    const request = { secure: false, header: (name: string) => name === 'host' ? 'attacker.example:8787' : undefined } as Request;
    const next = vi.fn() as NextFunction;
    productionSecurity(request, response, next);
    expect(response.status).toHaveBeenCalledWith(421);
    expect(next).not.toHaveBeenCalled();
    expect(headers.get('X-Frame-Options')).toBe('DENY');
  });

  it('accepts an explicitly allowed host without treating the port as part of it', () => {
    process.env.NODE_ENV = 'production';
    process.env.IMAIL_ALLOWED_HOSTS = 'mail.example.com';
    const { response } = responseDouble();
    const request = { secure: true, header: (name: string) => name === 'host' ? 'mail.example.com:443' : undefined } as Request;
    const next = vi.fn() as NextFunction;
    productionSecurity(request, response, next);
    expect(next).toHaveBeenCalledOnce();
    expect(response.setHeader).toHaveBeenCalledWith('Strict-Transport-Security', 'max-age=31536000');
  });

  it('keeps development origins out of production and validates configured origins', () => {
    expect(configuredCorsOrigins()).toEqual(['http://localhost:5173', 'http://127.0.0.1:5173']);
    process.env.NODE_ENV = 'production';
    expect(configuredCorsOrigins()).toEqual([]);
    process.env.CORS_ORIGIN = 'https://desktop.example.com/, https://web.example.com';
    expect(configuredCorsOrigins()).toEqual(['https://desktop.example.com', 'https://web.example.com']);
    process.env.CORS_ORIGIN = 'https://desktop.example.com/path';
    expect(() => configuredCorsOrigins()).toThrow('无效来源');
    process.env.CORS_ORIGIN = 'http://desktop.example.com';
    expect(() => configuredCorsOrigins()).toThrow('HTTPS');
  });

  it('accepts secure same-origin, loopback HTTP and configured cross-origin callers only', () => {
    process.env.NODE_ENV = 'production';
    process.env.CORS_ORIGIN = 'https://desktop.example.com';
    expect(requestOriginAllowed('https://mail.example.com', 'mail.example.com')).toBe(true);
    expect(requestOriginAllowed('https://mail.example.com', 'mail.example.com:443')).toBe(true);
    expect(requestOriginAllowed('http://mail.example.com', 'mail.example.com')).toBe(false);
    expect(requestOriginAllowed('http://127.0.0.1:8787', '127.0.0.1:8787')).toBe(true);
    expect(requestOriginAllowed('https://desktop.example.com', 'mail.example.com')).toBe(true);
    expect(requestOriginAllowed('https://attacker.example', 'mail.example.com')).toBe(false);
    expect(requestOriginAllowed('https://desktop.example.com/path', 'mail.example.com')).toBe(false);
  });
});
