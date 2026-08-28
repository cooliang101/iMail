import { DOMParser } from 'linkedom';
import { describe, expect, it } from 'vitest';
import type { TranslationPresentation } from '../translation';
import { buildBilingualEmailHtml } from './bilingual-email-html';

const presentation: TranslationPresentation = {
  document: {
    messageId: 'message-1',
    bodyHash: 'hash',
    segmentVersion: 1,
    omittedQuotedText: true,
    segments: [
      { id: 's-1', kind: 'paragraph', text: 'Hello team.' },
      { id: 's-2', kind: 'list-item', text: '- First\n- Second' },
    ],
  },
  artifact: {
    key: { userId: 'user-1', messageId: 'message-1', bodyHash: 'hash', sourceLanguage: 'en', targetLanguage: 'zh-Hans', profileId: 'edge', providerRevision: '1', segmentVersion: 1 },
    segments: [{ id: 's-1', text: '<团队好。>' }, { id: 's-2', text: '- 第一\n- 第二' }],
    createdAt: '2026-08-28T00:00:00Z',
    updatedAt: '2026-08-28T00:00:00Z',
  },
  targetLanguage: 'zh-Hans',
  busy: false,
};

const Parser = DOMParser as unknown as typeof globalThis.DOMParser;

describe('bilingual email HTML mapping', () => {
  it('keeps sanitized formatting and inserts escaped translations below matching blocks', () => {
    const result = buildBilingualEmailHtml('<p style="color:red">Hello <strong>team.</strong><script>bad()</script></p><ul><li>First</li><li>Second</li></ul><blockquote><p>Old reply</p></blockquote>', presentation, Parser);
    expect(result?.mappedSegmentCount).toBe(2);
    expect(result?.html).toContain('style="color:red"');
    expect(result?.html).toContain('<strong>team.</strong>');
    expect(result?.html).not.toContain('<script');
    expect(result?.html).toContain('&lt;团队好。&gt;');
    expect(result?.html.indexOf('Hello')).toBeLessThan(result?.html.indexOf('&lt;团队好。&gt;') ?? -1);
    expect(result?.html).toContain('<li>First<div');
    expect(result?.html).toContain('第一</div></li>');
    expect(result?.html).toContain('<blockquote><p>Old reply</p></blockquote>');
  });

  it('returns no partial HTML when stable segments cannot be mapped reliably', () => {
    expect(buildBilingualEmailHtml('<p>Different body</p>', presentation, Parser)).toBeUndefined();
  });

  it('adds inline pending placeholders before an artifact is available', () => {
    const pending = buildBilingualEmailHtml('<p>Hello team.</p><ul><li>First</li><li>Second</li></ul>', { ...presentation, artifact: undefined, busy: true }, Parser);
    expect(pending?.html.match(/aria-label="正在翻译此段"/g)).toHaveLength(2);
  });

  it('maps list items that wrap their text in paragraph elements', () => {
    const result = buildBilingualEmailHtml('<p>Hello team.</p><ul><li><p>First</p></li><li><p>Second</p></li></ul>', presentation, Parser);
    expect(result?.html).toContain('<p>First</p><div');
    expect(result?.html).toContain('第一</div></li>');
  });

  it('splits a multi-block translation back under corresponding HTML paragraphs', () => {
    const multiBlock = {
      ...presentation,
      document: { ...presentation.document, segments: [{ id: 's-1', kind: 'paragraph' as const, text: 'Hello team.\nWelcome aboard.' }] },
      artifact: { ...presentation.artifact!, segments: [{ id: 's-1', text: '团队好。\n欢迎加入。' }] },
    };
    const result = buildBilingualEmailHtml('<p>Hello team.</p><p>Welcome aboard.</p>', multiBlock, Parser);
    expect(result?.html).toMatch(/<p>Hello team\.<\/p><div[^>]*class="mail-inline-translation"[^>]*>/);
    expect(result?.html).toContain('团队好。</div><p>Welcome aboard.</p><div');
    expect(result?.html).toContain('欢迎加入。</div>');
  });
});
