import { DOMParser } from 'linkedom';
import { describe, expect, it } from 'vitest';
import type { Account, Message } from '../../types';
import { needsAttachmentReminder, replyHeaders, replyRecipients } from './reply-model';
import { signatureHtml } from './signature-node';

const parser = DOMParser as unknown as typeof globalThis.DOMParser;
const address = (address: string) => ({ name: '', address });
const accounts = [{ email: 'me@example.test' }, { email: 'work@example.test' }] as Account[];
const original = {
  from: address('sender@example.test'), replyTo: [address('reply@example.test')],
  to: [address('ME@example.test'), address('other@example.test'), address('reply@example.test')],
  cc: [address('WORK@example.test'), address('OTHER@example.test'), address('copy@example.test')],
  messageId: '<parent@example.test>', references: ['<root@example.test>'],
} as Message;

describe('reply composition', () => {
  it('uses Reply-To and removes all configured self addresses and duplicates across To/Cc', () => {
    expect(replyRecipients(original, accounts, true)).toEqual({ to: ['reply@example.test', 'other@example.test'], cc: ['copy@example.test'] });
    expect(replyRecipients(original, accounts, false)).toEqual({ to: ['reply@example.test'], cc: [] });
    expect(replyRecipients({ ...original, replyTo: [] }, accounts, false).to).toEqual(['sender@example.test']);
  });
  it('keeps reply ancestry, normalizes IDs and rejects injected headers', () => {
    expect(replyHeaders(original)).toEqual({ inReplyTo: ['<parent@example.test>'], references: ['<root@example.test>', '<parent@example.test>'] });
    expect(replyHeaders({ ...original, messageId: '<bad@example.test>\r\nBcc: hidden@example.test', references: ['invalid', 'root@example.test'] })).toEqual({ inReplyTo: [], references: ['<root@example.test>'] });
    expect(replyHeaders({ ...original, references: Array.from({ length: 110 }, (_, n) => `<${n}@example.test>`) }).references).toHaveLength(100);
  });
  it('warns for authored attachment mentions, not signatures or quoted history', () => {
    expect(needsAttachmentReminder('<p>请查收附件</p>', 0, parser)).toBe(true);
    expect(needsAttachmentReminder('<p>I attached the report.</p>', 0, parser)).toBe(true);
    expect(needsAttachmentReminder('<p>请查收附件</p>', 1, parser)).toBe(false);
    expect(needsAttachmentReminder('<p>谢谢</p><blockquote>请查收附件</blockquote><div data-compose-signature="a">Attachment support</div>', 0, parser)).toBe(false);
  });
  it('escapes signature markup and account identifiers', () => {
    const html = signatureHtml('a" onclick="bad', '<script>alert(1)</script>\nThanks');
    expect(html).not.toContain('<script>');
    expect(html).toContain('&lt;script&gt;');
    expect(signatureHtml('a', '')).toBe('');
  });
});
