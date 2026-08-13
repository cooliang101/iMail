import { describe, expect, it } from 'vitest';
import type { Account, Message } from '../../types';
import { appendMessagePage, applyMessageChanges, applyMessageStatsChanges, cacheMessageBody, messageMatchesQuery, messageTotalDelta, reconcileMessageCache } from './message-cache';

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

  it('appends a cursor page after live mail insertion without duplicating the boundary', () => {
    const first = message({ id: 'message-3', date: '2026-07-30T03:00:00.000Z' });
    const boundary = message({ id: 'message-2', date: '2026-07-30T02:00:00.000Z' });
    const live = message({ id: 'message-4', date: '2026-07-30T04:00:00.000Z' });
    const older = message({ id: 'message-1', date: '2026-07-30T01:00:00.000Z' });

    expect(appendMessagePage([live, first, boundary], [boundary, older]).map((item) => item.id))
      .toEqual(['message-4', 'message-3', 'message-2', 'message-1']);
  });

  it('bounds loaded message bodies with deterministic least-recently-used eviction', () => {
    const first = message({ id: 'message-1', text: 'first body', html: '<p>first</p>' });
    const second = message({ id: 'message-2', text: 'second body', html: '<p>second</p>' });
    const third = message({ id: 'message-3' });
    const cached = cacheMessageBody([first, second, third], { ...third, text: 'third body', html: '<p>third</p>' }, ['message-2', 'message-1'], 2);
    expect(cached.recentBodyIds).toEqual(['message-3', 'message-2']);
    expect(cached.messages.find((item) => item.id === 'message-1')).toMatchObject({ text: undefined, html: undefined });
    expect(cached.messages.find((item) => item.id === 'message-2')?.text).toBe('second body');
    expect(cached.messages.find((item) => item.id === 'message-3')?.text).toBe('third body');
  });
});
