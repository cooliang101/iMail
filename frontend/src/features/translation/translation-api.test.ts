import { beforeEach, describe, expect, it, vi } from 'vitest';

const { apiMock } = vi.hoisted(() => ({
  apiMock: vi.fn<(path: string, options?: RequestInit) => Promise<unknown>>(() => Promise.resolve({})),
}));
vi.mock('../../services', () => ({ api: apiMock }));

import { translationSettingsApi } from './translation-api';

describe('translation API cancellation', () => {
  beforeEach(() => apiMock.mockClear());

  it('passes AbortSignal through every message translation request', async () => {
    const controller = new AbortController();
    await translationSettingsApi.prepareMessage('message-1', { profileId: 'profile-1', targetLanguage: 'zh-Hans' }, controller.signal);
    await translationSettingsApi.executeMessage('message-1', { profileId: 'profile-1', targetLanguage: 'zh-Hans' }, controller.signal);
    await translationSettingsApi.completeMessage('message-1', { profileId: 'profile-1', targetLanguage: 'zh-Hans', segments: [] }, controller.signal);

    expect(apiMock).toHaveBeenCalledTimes(3);
    for (const [, options] of apiMock.mock.calls) expect(options).toMatchObject({ signal: controller.signal });
  });
});
