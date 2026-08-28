import { renderToStaticMarkup } from 'preact-render-to-string';
import { describe, expect, it } from 'vitest';
import { BilingualMessageBody } from './BilingualMessageBody';
import type { TranslationPresentation } from './types';

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
    segments: [{ id: 's-1', text: '团队好。' }, { id: 's-2', text: '- 第一\n- 第二' }],
    createdAt: '2026-08-28T00:00:00Z',
    updatedAt: '2026-08-28T00:00:00Z',
  },
  targetLanguage: 'zh-Hans',
  busy: false,
};

describe('BilingualMessageBody', () => {
  it('places each translated segment immediately after its source segment', () => {
    const html = renderToStaticMarkup(<BilingualMessageBody presentation={presentation} mode="bilingual" />);
    expect(html.indexOf('Hello team.')).toBeLessThan(html.indexOf('团队好。'));
    expect(html.indexOf('- First')).toBeLessThan(html.indexOf('- 第一'));
    expect(html).toContain('引用历史未翻译');
  });

  it('supports translation-only mode without rendering source text', () => {
    const html = renderToStaticMarkup(<BilingualMessageBody presentation={presentation} mode="translation" />);
    expect(html).not.toContain('Hello team.');
    expect(html).toContain('团队好。');
  });

  it('shows per-segment placeholders while translation is running', () => {
    const html = renderToStaticMarkup(<BilingualMessageBody presentation={{ ...presentation, artifact: undefined, busy: true }} mode="bilingual" />);
    expect(html.match(/正在翻译此段/g)).toHaveLength(2);
  });

  it('explains the safe reading layout for HTML messages and offers the original layout', () => {
    const html = renderToStaticMarkup(<BilingualMessageBody presentation={presentation} mode="bilingual" hasHtml onShowOriginal={() => undefined} />);
    expect(html).toContain('图片与复杂格式保留在原始邮件中');
    expect(html).toContain('查看原始排版');
  });
});
