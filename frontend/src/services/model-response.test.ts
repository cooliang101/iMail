import { describe, expect, it } from 'vitest';
import { isAccount, isMessage } from './model-response';

describe('domain response validation', () => {
  it('rejects accounts whose nested mailbox collection is null', () => {
    expect(isAccount({
      id: 'account-1', provider: 'gmail', email: 'a@example.com', displayName: 'A',
      group: '个人', groupIcon: 'folder', color: '#fff', status: 'connected', mailboxes: null,
    })).toBe(false);
  });

  it('rejects messages with nullable collections or invalid dates', () => {
    const base = {
      id: 'message-1', accountId: 'account-1', mailbox: 'INBOX', mailboxRole: 'inbox',
      from: { name: 'A', address: 'a@example.com', logo: { url: '/logo' } }, to: [],
      subject: '', preview: '', date: '2026-09-03T00:00:00Z', unread: true, flagged: false,
      hasAttachments: false, attachments: [], labels: [],
    };
    expect(isMessage({ ...base, labels: null })).toBe(false);
    expect(isMessage({ ...base, date: 'not-a-date' })).toBe(false);
    expect(isMessage(base)).toBe(true);
  });
});
