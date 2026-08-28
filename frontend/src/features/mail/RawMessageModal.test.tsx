import { renderToStaticMarkup } from 'preact-render-to-string';
import { describe, expect, it, vi } from 'vitest';
import type { Message } from '../../types';
import { RawMessageModal, rawMessageSource } from './RawMessageModal';

const message: Message = {
  id: 'message-1', accountId: 'account-1', mailbox: 'INBOX', mailboxRole: 'inbox',
  from: { name: 'Sender', address: 'sender@example.com', logo: { url: '' } },
  to: [{ name: 'Owner', address: 'owner@example.com' }], subject: 'Raw source', preview: '',
  text: 'Fallback text', html: '<script>alert(1)</script><img src="https://tracker.example/pixel">&amp;',
  date: '2026-08-28T00:00:00Z', unread: false, flagged: false, hasAttachments: false,
  attachments: [], labels: [],
};

describe('RawMessageModal', () => {
  it('returns the stored body without normalizing or sanitizing it', () => {
    expect(rawMessageSource(message)).toBe(message.html);
    expect(rawMessageSource({ text: '  exact\r\ntext  ' })).toBe('  exact\r\ntext  ');
  });

  it('escapes active email markup and renders it only as preformatted text', () => {
    const html = renderToStaticMarkup(<RawMessageModal message={message} onClose={vi.fn()} />);
    expect(html).not.toContain('<script>');
    expect(html).not.toContain('<img src=');
    expect(html).toContain('&lt;script>alert(1)&lt;/script>');
    expect(html).toContain('&lt;img src=&quot;https://tracker.example/pixel&quot;>&amp;amp;');
    expect(html).toContain('不会渲染标签、执行脚本或加载邮件中的任何资源');
  });
});
