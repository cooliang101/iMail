import { describe, expect, it, vi } from 'vitest';
import { createApiTransport } from './api';

describe('API transport', () => {
  it('supports an injected base URL without changing feature paths', async () => {
    const fetcher = vi.fn(async () => new Response(JSON.stringify({ ok: true }), { status: 200, headers: { 'Content-Type': 'application/json' } }));
    const transport = createApiTransport({ baseUrl: 'http://127.0.0.1:8787', fetcher });
    await expect(transport.request<{ ok: boolean }>('/api/health')).resolves.toEqual({ ok: true });
    expect(fetcher).toHaveBeenCalledWith('http://127.0.0.1:8787/api/health', expect.objectContaining({ credentials: 'include' }));
  });

  it('returns undefined for an empty successful response', async () => {
    const transport = createApiTransport({ fetcher: vi.fn(async () => new Response(null, { status: 204 })) });
    await expect(transport.request('/api/auth/logout', { method: 'POST' })).resolves.toBeUndefined();
  });
});
