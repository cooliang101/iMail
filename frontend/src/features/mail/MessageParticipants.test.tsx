import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'preact-render-to-string';
import type { Message } from '../../types';
import { MessageParticipants } from './MessageParticipants';

const message: Message = {
  id: 'message-1',
  accountId: 'account-1',
  mailbox: 'INBOX',
  mailboxRole: 'inbox',
  from: { name: 'Sender', address: 'sender@example.com', logo: { url: '' } },
  to: [
    { name: 'Owner', address: 'owner@example.com' },
    { name: 'Duplicate owner', address: 'OWNER@example.com' },
    { name: 'Archive', address: 'archive@example.com' },
  ],
  subject: 'Subject',
  preview: 'Preview',
  date: '2026-08-27T00:00:00Z',
  unread: false,
  flagged: false,
  hasAttachments: false,
  attachments: [],
  labels: [],
};

describe('MessageParticipants', () => {
  it('renders every distinct recipient as an independent accessible trigger', () => {
    const html = renderToStaticMarkup(<MessageParticipants message={message} contacts={[]} color="#168f78" onCompose={vi.fn()} />);
    expect(html).toContain('aria-label="查看收件人 Owner &lt;owner@example.com>"');
    expect(html).toContain('aria-label="查看收件人 Archive &lt;archive@example.com>"');
    expect((html.match(/class="recipient-address"/g) ?? [])).toHaveLength(2);
    expect(html).not.toContain('Duplicate owner');
  });
});
