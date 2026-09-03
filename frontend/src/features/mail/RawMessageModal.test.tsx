import { renderToStaticMarkup } from 'preact-render-to-string';
import { describe, expect, it, vi } from 'vitest';
import type { Message } from '../../types';
import { decodeRawMessageSource, RawMessageModal, rawMessageSource } from './RawMessageModal';
import { PlatformProvider } from '../../platform/runtime';
import type { PlatformRuntime } from '../../platform/types';
import { fetchRawMessageBlob, rawMessageDownloadPath, rawMessageFilename } from './raw-message-download';

const runtime: PlatformRuntime = {
  kind: 'web', openExternal: vi.fn(), saveDownload: vi.fn(), saveImage: vi.fn(), saveText: vi.fn(), prepareNotifications: vi.fn(async () => false),
  notify: vi.fn(), subscribeNotificationClicks: vi.fn(() => () => undefined),
};

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

  it('builds a safe EML filename and exact source download path', () => {
    expect(rawMessageFilename(' Quarterly: report? ')).toBe('Quarterly_ report_.eml');
    expect(rawMessageFilename('...')).toBe('原始邮件.eml');
    expect(rawMessageFilename('CON')).toBe('_CON.eml');
    expect(rawMessageFilename(`${'x'.repeat(95)} .ignored`)).toBe(`${'x'.repeat(95)}.eml`);
    expect(rawMessageDownloadPath('message 1')).toBe('/api/messages/message%201/source/download');
  });

  it('accepts only a successful RFC 822 response before saving', async () => {
    const exact = new Uint8Array([0x46, 0x72, 0x6f, 0x6d, 0x3a, 0xff]);
    const blob = await fetchRawMessageBlob('/source', vi.fn(async () => new Response(exact, { headers: { 'Content-Type': 'message/rfc822' } })));
    expect(new Uint8Array(await blob.arrayBuffer())).toEqual(exact);
    await expect(fetchRawMessageBlob('/source', vi.fn(async () => new Response(JSON.stringify({ error: '原件不存在' }), { status: 404, headers: { 'Content-Type': 'application/json' } })))).rejects.toThrow('原件不存在');
    await expect(fetchRawMessageBlob('/source', vi.fn(async () => new Response('wrong', { headers: { 'Content-Type': 'text/plain' } })))).rejects.toThrow('格式不正确');
  });

  it('escapes active email markup and renders it only as preformatted text', () => {
    const html = renderToStaticMarkup(<PlatformProvider runtime={runtime}><RawMessageModal message={message} onClose={vi.fn()} /></PlatformProvider>);
    expect(html).not.toContain('<script>');
    expect(html).not.toContain('<img src=');
    expect(html).toContain('正在读取原始邮件');
    expect(html).toContain('不会渲染 HTML、执行脚本、加载图片或访问邮件中的任何资源');
    expect(html).toContain('aria-label="下载原始邮件"');
    expect(html).not.toContain('>下载原始邮件</button>');
    expect(html).toContain('raw-message-source app-scrollbar');
  });
});
