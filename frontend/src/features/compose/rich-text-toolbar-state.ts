import type { Editor } from '@tiptap/core';

export type RichTextToolbarState = {
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strike: boolean;
  heading: boolean;
  bullet: boolean;
  ordered: boolean;
  quote: boolean;
  alignCenter: boolean;
  link: boolean;
  canUndo: boolean;
  canRedo: boolean;
};

export function selectRichTextToolbarState({ editor }: { editor: Editor | null }): RichTextToolbarState | null {
  if (!editor || editor.isDestroyed) return null;
  try {
    const commands = editor.can();
    return {
      bold: editor.isActive('bold'),
      italic: editor.isActive('italic'),
      underline: editor.isActive('underline'),
      strike: editor.isActive('strike'),
      heading: editor.isActive('heading', { level: 2 }),
      bullet: editor.isActive('bulletList'),
      ordered: editor.isActive('orderedList'),
      quote: editor.isActive('blockquote'),
      alignCenter: editor.isActive({ textAlign: 'center' }),
      link: editor.isActive('link'),
      canUndo: commands.undo(),
      canRedo: commands.redo(),
    };
  } catch {
    // The editor can be destroyed between the external-store snapshot and selector execution.
    return null;
  }
}
