import { describe, expect, it } from 'vitest';
import { defaultShortcutBindings, isBrowserRefreshShortcut, loadShortcutBindings, shortcutConflict, shortcutFromEvent, shortcutLabel, shortcutMatches } from './shortcut-model';

describe('shortcut model', () => {
  it('normalizes platform modifier keys and matches exact combinations', () => {
    const event = { key: 'k', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false };
    expect(shortcutFromEvent(event)).toBe('Mod+K');
    expect(shortcutMatches(event, 'Mod+K')).toBe(true);
    expect(shortcutMatches({ ...event, shiftKey: true }, 'Mod+K')).toBe(false);
  });

  it('loads defaults around saved overrides and detects conflicts', () => {
    const bindings = loadShortcutBindings({ getItem: () => JSON.stringify({ focusSearch: 'Mod+F' }) });
    expect(bindings.focusSearch).toBe('Mod+F');
    expect(bindings.compose).toBe(defaultShortcutBindings.compose);
    expect(shortcutConflict(bindings, 'compose', 'Mod+F')?.id).toBe('focusSearch');
  });

  it('uses intuitive mail navigation and action defaults', () => {
    expect(defaultShortcutBindings).toMatchObject({
      sync: 'Mod+Shift+R',
      nextMessage: 'ArrowRight',
      previousMessage: 'ArrowLeft',
      archive: 'A',
      delete: 'Delete',
    });
    expect(shortcutLabel(defaultShortcutBindings.nextMessage)).toBe('→');
    expect(shortcutLabel(defaultShortcutBindings.previousMessage)).toBe('←');
  });

  it('leaves exact browser refresh available and migrates the legacy sync binding', () => {
    expect(isBrowserRefreshShortcut({ key: 'r', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false })).toBe(true);
    expect(isBrowserRefreshShortcut({ key: 'R', ctrlKey: false, metaKey: true, altKey: false, shiftKey: true })).toBe(false);
    const bindings = loadShortcutBindings({ getItem: () => JSON.stringify({ sync: 'Mod+R' }) });
    expect(bindings.sync).toBe(defaultShortcutBindings.sync);
    expect(loadShortcutBindings({ getItem: () => JSON.stringify({ sync: 'Mod+Shift+S' }) }).sync).toBe('Mod+Shift+S');
  });
});
