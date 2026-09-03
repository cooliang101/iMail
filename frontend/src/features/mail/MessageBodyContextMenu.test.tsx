import { renderToStaticMarkup } from 'preact-render-to-string';
import { describe, expect, it, vi } from 'vitest';
import type { PlatformRuntime } from '../../platform/types';
import { MessageBodyContextMenu } from './MessageBodyContextMenu';

const actions = {
  onClose: vi.fn(), onCopy: vi.fn(), onDownloadRaw: vi.fn(), onSaveBody: vi.fn(), onTranslate: vi.fn(),
  onToggleBodyView: vi.fn(), onOpen: vi.fn(), onShare: vi.fn(), onSaveImage: vi.fn(),
};
const runtime: PlatformRuntime = {
  kind: 'web', openExternal: vi.fn(), share: vi.fn(), saveDownload: vi.fn(), saveImage: vi.fn(), saveText: vi.fn(), prepareNotifications: vi.fn(async () => false),
  notify: vi.fn(), subscribeNotificationClicks: vi.fn(() => () => undefined),
};

function menu(target: Parameters<typeof MessageBodyContextMenu>[0]['target'], overrides: Partial<Parameters<typeof MessageBodyContextMenu>[0]> = {}) {
  return renderToStaticMarkup(<MessageBodyContextMenu x={10} y={10} target={target} platform={runtime} bodyView="source" hasHtml translationActive={false} {...actions} {...overrides} />);
}

describe('MessageBodyContextMenu', () => {
  it('keeps selected-text actions intentionally minimal', () => {
    const html = menu({ kind: 'selection', text: 'selected' });
    expect(html).toContain('所选文本');
    expect(html).toContain('复制');
    expect(html).not.toContain('下载原始邮件');
  });

  it('offers safe web-link actions only when supported', () => {
    const html = menu({ kind: 'link', href: 'https://example.test/path' });
    expect(html).toContain('在新窗口打开');
    expect(html).toContain('复制链接');
    expect(html).toContain('分享链接');
    const linkedImage = menu({ kind: 'link', href: 'https://example.test/path', image: { src: 'https://cdn.example.test/banner.png', alt: 'Banner' } });
    expect(linkedImage).toContain('打开图片');
    expect(linkedImage).toContain('复制图片地址');
  });

  it('offers body actions and hides an ineffective render toggle during translation', () => {
    const html = menu({ kind: 'body' });
    expect(html).toContain('复制当前显示内容');
    expect(html).toContain('下载原始邮件（.eml）');
    expect(html).toContain('保存当前显示内容（.txt）');
    expect(html).toContain('翻译邮件…');
    expect(html).toContain('切换到原始样式');
    expect(menu({ kind: 'body' }, { translationActive: true })).not.toContain('切换到原始样式');
  });

  it('keeps sharing and remote-image saving available in the desktop runtime', () => {
    const desktop = { ...runtime, kind: 'tauri' as const };
    const html = menu({ kind: 'link', href: 'https://example.test/path', image: { src: 'https://cdn.example.test/banner.png', alt: 'Banner' } }, { platform: desktop });
    expect(html).toContain('分享链接');
    expect(html).toContain('保存图片');
  });
});
