import { describe, expect, it } from 'vitest';
import type { Message } from '../../types';
import { reconcileMessageCache } from './message-cache';

function message(overrides: Partial<Message> = {}): Message {
  return {
    id: 'message-1', accountId: 'account-1', mailbox: 'INBOX', mailboxRole: 'inbox',
    from: { name: 'Sender', address: 'sender@example.com', logo: { url: '/logo' } }, to: [], subject: 'Subject', preview: 'Preview',
    date: '2026-07-30T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [], labels: [], ...overrides,
  };
}

describe('message cache reconciliation', () => {
  it('keeps the existing reference when background data is unchanged', () => {
    const current = [message()];
    expect(reconcileMessageCache(current, [message()])).toBe(current);
  });

  it('applies changed summaries without discarding the open message body', () => {
    const current = [message({ text: 'Loaded body', html: '<p>Loaded body</p>' })];
    const reconciled = reconcileMessageCache(current, [message({ unread: false })]);
    expect(reconciled).not.toBe(current);
    expect(reconciled[0]).toMatchObject({ unread: false, text: 'Loaded body', html: '<p>Loaded body</p>' });
  });

  it('prepends new mail while preserving unchanged row identities', () => {
    const existing = message();
    const incoming = [message({ id: 'message-2', subject: 'New message' }), message()];
    const reconciled = reconcileMessageCache([existing], incoming);
    expect(reconciled.map((item) => item.id)).toEqual(['message-2', 'message-1']);
    expect(reconciled[1]).toBe(existing);
  });
});
