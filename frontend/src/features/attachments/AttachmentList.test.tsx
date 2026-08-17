import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'preact-render-to-string';
import { PlatformProvider } from '../../platform/runtime';
import type { PlatformRuntime } from '../../platform/types';
import { AttachmentList } from './AttachmentList';

const runtime: PlatformRuntime = {
  kind: 'web',
  openExternal: vi.fn(),
  saveDownload: vi.fn(),
  prepareNotifications: vi.fn(async () => false),
  notify: vi.fn(),
  subscribeNotificationClicks: vi.fn(() => () => undefined),
};

describe('AttachmentList', () => {
  it('offers preview only for supported attachment families while retaining download', () => {
    const html = renderToStaticMarkup(<PlatformProvider runtime={runtime}><AttachmentList messageId="message 1" attachments={[
      { filename: 'photo.png', contentType: 'image/png', size: 2048, index: 0 },
      { filename: 'slides.pptx', contentType: 'application/vnd.openxmlformats-officedocument.presentationml.presentation', size: 4096, index: 1 },
      { filename: 'notes.txt', contentType: 'text/plain', size: 128, index: 2 },
    ]} /></PlatformProvider>);
    expect((html.match(/>查看</g) ?? []).length).toBe(2);
    expect((html.match(/>下载</g) ?? []).length).toBe(3);
  });
});
