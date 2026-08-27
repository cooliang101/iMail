import { describe, expect, it, vi } from 'vitest';
import { defaultShortcutBindings, isBrowserRefreshShortcut, loadShortcutBindings, preventBrowserRefresh, shortcutConflict, shortcutDefinitions, shortcutFromEvent, shortcutLabel, shortcutMatches } from './shortcut-model';

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
    expect(shortcutDefinitions.find((item) => item.id === 'sync')).toMatchObject({ label: '立即同步', description: '刷新当前邮件范围' });
  });

  it('recognizes application reload shortcuts so the desktop shell can block them', () => {
    expect(isBrowserRefreshShortcut({ key: 'r', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false })).toBe(true);
    expect(isBrowserRefreshShortcut({ key: 'R', ctrlKey: false, metaKey: true, altKey: false, shiftKey: true })).toBe(true);
    expect(isBrowserRefreshShortcut({ key: 'F5', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false })).toBe(true);
    expect(isBrowserRefreshShortcut({ key: 'r', ctrlKey: true, metaKey: false, altKey: true, shiftKey: false })).toBe(false);
    const preventDefault = vi.fn();
    const stopImmediatePropagation = vi.fn();
    expect(preventBrowserRefresh({ key: 'r', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, preventDefault, stopImmediatePropagation })).toBe(true);
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(stopImmediatePropagation).toHaveBeenCalledOnce();
  });

  it('migrates the legacy sync binding away from the reload shortcut', () => {
    const bindings = loadShortcutBindings({ getItem: () => JSON.stringify({ sync: 'Mod+R' }) });
    expect(bindings.sync).toBe(defaultShortcutBindings.sync);
    expect(loadShortcutBindings({ getItem: () => JSON.stringify({ sync: 'Mod+Shift+S' }) }).sync).toBe('Mod+Shift+S');
  });
});
