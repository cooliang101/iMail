import { renderToStaticMarkup } from 'preact-render-to-string';
import { describe, expect, it, vi } from 'vitest';
import { TranslationReaderControl, translationDisplayOptions } from './TranslationReaderControl';

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

  it('offers an explicit translation-off mode without duplicating the original-layout control', () => {
    expect(translationDisplayOptions).toEqual([
      { mode: 'bilingual', label: '双语' },
      { mode: 'translation', label: '仅译文' },
      { mode: 'original', label: '关闭翻译' },
    ]);
    expect(translationDisplayOptions.some(({ label }) => label.includes('原始排版'))).toBe(false);
  });
});
