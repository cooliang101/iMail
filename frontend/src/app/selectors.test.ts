import { describe, expect, it } from 'vitest';
import { buildMessageQuery } from '../features/mail/message-query';

describe('buildMessageQuery', () => {
  it('adds exact participant addresses without replacing the other active filters', () => {
    const query = new URLSearchParams(buildMessageQuery({
      accountFilter: 'account-1',
      groupFilter: null,
      search: 'invoice',
      view: 'inbox',
      mailFilter: 'unread',
      activeLabel: 'work',
      activeMailbox: null,
      participantFilters: {
        sender: { name: 'Wayne', address: ' Sender+Alerts@Example.com ' },
        recipient: { name: '', address: 'alias@icloud.com' },
      },
    }));

    expect(Object.fromEntries(query)).toMatchObject({
      accountId: 'account-1',
      q: 'invoice',
      mailboxRole: 'inbox',
      unread: 'true',
      label: 'work',
      sender: 'Sender+Alerts@Example.com',
      recipient: 'alias@icloud.com',
    });
  });
});
