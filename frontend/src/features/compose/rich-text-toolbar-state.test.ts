import { describe, expect, it, vi } from 'vitest';
import type { Editor } from '@tiptap/core';
import { selectRichTextToolbarState } from './rich-text-toolbar-state';

describe('rich text toolbar state', () => {
  it('does not read commands before the editor view is ready', () => {
    const can = vi.fn();
    const editor = { isDestroyed: true, can } as unknown as Editor;
    expect(selectRichTextToolbarState({ editor })).toBeNull();
    expect(can).not.toHaveBeenCalled();
  });

  it('survives an editor destroyed during snapshot selection', () => {
    const editor = { isDestroyed: false, can: () => { throw new TypeError('editor view is null'); } } as unknown as Editor;
    expect(selectRichTextToolbarState({ editor })).toBeNull();
  });
});
