import { describe, expect, it } from 'vitest';
import { canPreviewAttachment, formatAttachmentSize, previewContentPath } from './attachment-api';

describe('attachment preview helpers', () => {
  it('only advertises supported preview families', () => {
    expect(canPreviewAttachment({ filename: 'photo.bin', contentType: 'image/png' })).toBe(true);
    expect(canPreviewAttachment({ filename: 'movie.webm', contentType: '' })).toBe(true);
    expect(canPreviewAttachment({ filename: 'README.md', contentType: 'text/markdown' })).toBe(true);
    expect(canPreviewAttachment({ filename: 'data.json', contentType: 'application/json' })).toBe(true);
    expect(canPreviewAttachment({ filename: 'slides.pptx', contentType: 'application/vnd.openxmlformats-officedocument.presentationml.presentation' })).toBe(false);
    expect(canPreviewAttachment({ filename: 'vector.svg', contentType: 'image/svg+xml' })).toBe(false);
  });

  it('builds encoded session paths and readable sizes', () => {
    expect(previewContentPath('preview id', 'entry/id')).toBe('/api/attachment-previews/preview%20id/archive/entries/entry%2Fid');
    expect(formatAttachmentSize(1536)).toBe('2 KB');
  });
});
