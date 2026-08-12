import { describe, expect, it, vi } from 'vitest';
import { resolveSenderLogoSource } from './SenderAvatar';

describe('sender avatar source', () => {
  it('keeps the server URL in the web client', async () => {
    const readBinary = vi.fn();
    await expect(resolveSenderLogoSource('/api/contacts/logo?address=a%40example.com', false, readBinary)).resolves.toBe('/api/contacts/logo?address=a%40example.com');
    expect(readBinary).not.toHaveBeenCalled();
  });

  it('loads protected logos through the desktop bridge', async () => {
    const readBinary = vi.fn(async () => new Uint8Array([137, 80, 78, 71]));
    const createObjectUrl = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:sender-logo');
    await expect(resolveSenderLogoSource('/api/contacts/logo?address=b%40example.com', true, readBinary)).resolves.toBe('blob:sender-logo');
    expect(readBinary).toHaveBeenCalledWith('/api/contacts/logo?address=b%40example.com');
    createObjectUrl.mockRestore();
  });

  it('does not permanently cache a failed desktop logo request', async () => {
    const readBinary = vi.fn()
      .mockRejectedValueOnce(new Error('图片加载失败：503'))
      .mockResolvedValueOnce(new Uint8Array([137, 80, 78, 71]));
    const createObjectUrl = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:retried-sender-logo');
    const logoUrl = '/api/contacts/logo?address=retry%40example.com';

    await expect(resolveSenderLogoSource(logoUrl, true, readBinary)).rejects.toThrow('503');
    await expect(resolveSenderLogoSource(logoUrl, true, readBinary)).resolves.toBe('blob:retried-sender-logo');
    expect(readBinary).toHaveBeenCalledTimes(2);
    createObjectUrl.mockRestore();
  });
});
