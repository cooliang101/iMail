import { renderToStaticMarkup } from 'preact-render-to-string';
import { describe, expect, it, vi } from 'vitest';
import { abortTranslationRequest, TranslationReaderControl, translationContextPosition, translationDisplayOptions } from './TranslationReaderControl';

const props = {
  messageId: 'message-1',
  displayMode: 'bilingual' as const,
  onDisplayModeChange: vi.fn(),
  onPresentationChange: vi.fn(),
  onDismiss: vi.fn(),
};

describe('TranslationReaderControl', () => {
  it('renders as a compact dismissible popover', () => {
    const html = renderToStaticMarkup(<TranslationReaderControl {...props} open />);
    expect(html).toContain('role="dialog"');
    expect(html).toContain('id="mail-translation-popover"');
    expect(html).toContain('收起翻译设置');
    expect(html).not.toContain(' hidden');
  });

  it('stays mounted while visually collapsed so translation state can be preserved', () => {
    const html = renderToStaticMarkup(<TranslationReaderControl {...props} open={false} />);
    expect(html).toContain('hidden');
  });

  it('opens the service and target selector at the context-menu position', () => {
    const html = renderToStaticMarkup(<TranslationReaderControl {...props} open contextPoint={{ x: 120, y: 240 }} />);
    expect(html).toContain('mail-translation-control is-context');
    expect(html).toContain('left:120px');
    expect(html).toContain('top:240px');
    expect(html).toContain('选择服务与目标语言');
  });

  it('keeps a context-positioned selector inside the viewport', () => {
    expect(translationContextPosition({ x: 900, y: 700 }, { width: 340, height: 300 }, { width: 1000, height: 800 })).toEqual({ x: 652, y: 492 });
  });

  it('aborts an active request and invalidates its generation', () => {
    const controller = new AbortController();
    expect(abortTranslationRequest(controller, 4)).toBe(5);
    expect(controller.signal.aborted).toBe(true);
  });

  it('offers an explicit translation-off mode without duplicating the original-layout control', () => {
    expect(translationDisplayOptions).toEqual([
      { mode: 'bilingual', label: '双语' },
      { mode: 'translation', label: '仅译文' },
      { mode: 'original', label: '关闭翻译' },
    ]);
    expect(translationDisplayOptions.some(({ label }) => label.includes('原始排版'))).toBe(false);
  });
});
