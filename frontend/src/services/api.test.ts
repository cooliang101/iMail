import { describe, expect, it, vi } from 'vitest';
import { createApiTransport, userFacingErrorMessage } from './api';

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

  it('turns malformed successful responses into a contextual recoverable error', async () => {
    const transport = createApiTransport({ fetcher: vi.fn(async () => new Response('<html>broken</html>', { status: 200 })) });
    await expect(transport.request('/api/messages?limit=60')).rejects.toThrow('iMail 收到的数据格式不正确');
  });

  it('keeps runtime and route details out of user-facing errors', () => {
    expect(userFacingErrorMessage('Tauri 直连尚未映射该领域操作：GET /api/providers'))
      .toBe('iMail 暂时无法完成这项操作，请更新应用后重试');
    expect(userFacingErrorMessage('初始化嵌入式 Rust 服务失败'))
      .toBe('初始化 iMail 本地服务失败');
  });
});
