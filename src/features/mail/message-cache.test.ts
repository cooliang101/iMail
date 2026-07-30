import { describe, expect, it } from 'vitest';
import type { Account, Message } from '../../types';
import { applyMessageChanges, applyMessageStatsChanges, messageMatchesQuery, messageTotalDelta, reconcileMessageCache } from './message-cache';

function message(overrides: Partial<Message> = {}): Message {
  return {
    id: 'message-1', accountId: 'account-1', mailbox: 'INBOX', mailboxRole: 'inbox',
    from: { name: 'Sender', address: 'sender@example.com', logo: { url: '/logo' } }, to: [], subject: 'Subject', preview: 'Preview',
    date: '2026-07-30T00:00:00.000Z', unread: true, flagged: false, hasAttachments: false, attachments: [], labels: [], ...overrides,
  };
}

const account = { id: 'account-1', group: '个人', mailboxes: [{ path: 'INBOX', name: '收件箱' }] } as Account;

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

  it('applies SSE deltas using the active mailbox query without another list request', () => {
    const existing = message();
    const incoming = message({ id: 'message-2', subject: 'New message', date: '2026-07-30T01:00:00.000Z' });
    const changes = [{ after: incoming }, { before: existing, after: { ...existing, unread: false } }];
    const query = 'mailboxRole=inbox&unread=true';

    expect(messageMatchesQuery(incoming, query, [account])).toBe(true);
    expect(applyMessageChanges([existing], changes, query, [account])).toEqual([incoming]);
    expect(messageTotalDelta(changes, query, [account])).toBe(0);
    expect(applyMessageStatsChanges({ total: 1, unread: 1, byAccount: [{ accountId: account.id, total: 1, unread: 1 }], byGroup: [{ group: account.group, total: 1, unread: 1 }] }, changes, [account]))
      .toMatchObject({ total: 2, unread: 1, byAccount: [{ total: 2, unread: 1 }], byGroup: [{ total: 2, unread: 1 }] });
  });
});
