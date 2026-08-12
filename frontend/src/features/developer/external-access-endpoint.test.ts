import { describe, expect, it, vi } from 'vitest';
import { resolveExternalAccessBaseUrl } from './external-access-endpoint';

describe('external access endpoint', () => {
  it('starts the desktop loopback HTTP adapter in local application mode', async () => {
    const invoker = vi.fn(async () => ({ baseUrl: 'http://127.0.0.1:43123' }));

    await expect(resolveExternalAccessBaseUrl({ desktop: true, mode: 'local', invoker })).resolves.toBe('http://127.0.0.1:43123');
    expect(invoker).toHaveBeenCalledWith('desktop_start_external_http');
  });
});
