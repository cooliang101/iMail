import { describe, expect, it } from 'vitest';
import { defaultCustomTheme } from './theme-model';
import { contrastText, customBrandVariants, customThemeCssVariables, mixHex } from './theme-runtime';

describe('custom theme runtime', () => {
  it('derives deterministic Fluent and CSS colors', () => {
    expect(mixHex('#000000', '#ffffff', .5)).toBe('#808080');
    expect(customBrandVariants('#336699')[70]).toBe('#336699');
    expect(customThemeCssVariables(defaultCustomTheme)).toMatchObject({
      '--color-accent': '#d06f52', '--color-canvas': '#e9edf4', '--color-reader-surface': '#f9fafe',
      '--color-mail-body': '#faf6f6', '--color-message-selected': '#f8e8e2', '--color-rail-accent': '#d06f52',
      '--color-rail-control-active': '#f8e8e2', '--color-dialog-control-selected': '#f8e8e2',
      '--color-overlay-scrim': '#20283a66',
    });
  });

  it('chooses readable inverse text for light and dark surfaces', () => {
    expect(contrastText('#ffffff')).toBe('#17201e');
    expect(contrastText('#101820')).toBe('#ffffff');
  });
});
