import { describe, expect, it } from 'vitest';
import { defaultShortcutBindings, isBrowserRefreshShortcut, loadShortcutBindings, shortcutConflict, shortcutFromEvent, shortcutMatches } from './shortcut-model';

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

  it('leaves browser refresh available and migrates the legacy sync binding', () => {
    expect(defaultShortcutBindings.sync).toBe('');
    expect(isBrowserRefreshShortcut({ key: 'r', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false })).toBe(true);
    expect(isBrowserRefreshShortcut({ key: 'R', ctrlKey: false, metaKey: true, altKey: false, shiftKey: true })).toBe(true);
    const bindings = loadShortcutBindings({ getItem: () => JSON.stringify({ sync: 'Mod+R' }) });
    expect(bindings.sync).toBe('');
    expect(loadShortcutBindings({ getItem: () => JSON.stringify({ sync: 'Mod+Shift+S' }) }).sync).toBe('Mod+Shift+S');
  });
});
