import { describe, expect, it } from 'vitest';
import { defaultShortcutBindings, loadShortcutBindings, shortcutConflict, shortcutFromEvent, shortcutMatches } from './shortcut-model';

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
});
