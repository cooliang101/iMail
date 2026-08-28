import { describe, expect, it, vi } from 'vitest';
import type { Account } from '../../types';
import { loadPrivacyMailboxes } from './privacy-mailboxes';

const account = (id: string, provider: Account['provider']): Account => ({
  id,
  provider,
  email: `${id}@example.com`,
  displayName: id,
  group: '',
  groupIcon: 'folder',
  color: '#000000',
  status: 'connected',
  mailboxes: [],
});

describe('privacy mailbox loading', () => {
  it('keeps successful addresses and reports failed iCloud accounts', async () => {
    const request = vi.fn(async (path: string) => {
      if (path.includes('icloud-b')) throw new Error('offline');
      return { addresses: [{ anonymousId: 'hme-1', email: 'private@icloud.com', label: '注册', note: '', forwardToEmail: 'icloud-a@example.com', active: true, origin: 'example.com' }] };
    });

    const result = await loadPrivacyMailboxes([
      account('icloud-a', 'icloud'),
      account('gmail-a', 'gmail'),
      account('icloud-b', 'icloud'),
    ], request);

    expect(request).toHaveBeenCalledTimes(2);
    expect(result.failedCount).toBe(1);
    expect(result.mailboxes).toHaveLength(1);
    expect(result.mailboxes[0].ownerEmail).toBe('icloud-a@example.com');
  });
});
