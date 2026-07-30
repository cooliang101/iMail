import { describe, expect, it } from 'vitest';
import { defaultCustomTheme } from './theme-model';
import { contrastText, customBrandVariants, customThemeCssVariables, mixHex } from './theme-runtime';

describe('custom theme runtime', () => {
  it('derives deterministic Fluent and CSS colors', () => {
    expect(mixHex('#000000', '#ffffff', .5)).toBe('#808080');
    expect(customBrandVariants('#336699')[70]).toBe('#336699');
    expect(customThemeCssVariables(defaultCustomTheme)).toMatchObject({ '--color-accent': '#d06f52', '--color-canvas': '#e9edf4' });
  });

  it('chooses readable inverse text for light and dark surfaces', () => {
    expect(contrastText('#ffffff')).toBe('#17201e');
    expect(contrastText('#101820')).toBe('#ffffff');
  });
});
