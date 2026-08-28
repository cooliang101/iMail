import { renderToStaticMarkup } from 'preact-render-to-string';
import { describe, expect, it, vi } from 'vitest';
import type { Message } from '../../types';
import { decodeRawMessageSource, RawMessageModal, rawMessageSource } from './RawMessageModal';

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

  it('decodes RFC 822 response bytes for plain-text display', () => {
    const source = 'From: sender@example.com\r\nSubject: Hello\r\n\r\nBody';
    expect(decodeRawMessageSource(btoa(source))).toBe(source);
  });

  it('escapes active email markup and renders it only as preformatted text', () => {
    const html = renderToStaticMarkup(<RawMessageModal message={message} onClose={vi.fn()} />);
    expect(html).not.toContain('<script>');
    expect(html).not.toContain('<img src=');
    expect(html).toContain('正在读取原始邮件');
    expect(html).toContain('不会渲染 HTML、执行脚本、加载图片或访问邮件中的任何资源');
  });
});
