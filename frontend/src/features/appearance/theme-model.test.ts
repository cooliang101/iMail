import { describe, expect, it } from 'vitest';
import { defaultCustomTheme, defaultThemeId, normalizeCustomTheme, normalizeThemeId, parseCustomThemeJson } from './theme-model';

describe('theme model', () => {
  it('accepts every supported theme', () => {
    expect(normalizeThemeId('mint-fresh')).toBe('mint-fresh');
    expect(normalizeThemeId('tech')).toBe('tech');
    expect(normalizeThemeId('business-blue')).toBe('business-blue');
    expect(normalizeThemeId('soft-neubrutalism')).toBe('soft-neubrutalism');
    expect(normalizeThemeId('custom')).toBe('custom');
  });

  it('falls back safely for removed or malformed themes', () => {
    expect(normalizeThemeId('imail-light')).toBe('mint-fresh');
    expect(normalizeThemeId(null)).toBe(defaultThemeId);
  });

  it('normalizes partial custom themes without accepting arbitrary values', () => {
    expect(normalizeCustomTheme({ name: '  Ocean  ', accent: '#ABCDEF', canvas: 'url(evil)', shadow: 'wild' })).toMatchObject({
      name: 'Ocean', accent: '#abcdef', canvas: defaultCustomTheme.canvas, shadow: defaultCustomTheme.shadow,
    });
  });

  it('requires a complete valid AI theme JSON', () => {
    expect(parseCustomThemeJson(JSON.stringify(defaultCustomTheme)).theme).toEqual(defaultCustomTheme);
    expect(parseCustomThemeJson(JSON.stringify({ ...defaultCustomTheme, accent: 'red' })).error).toContain('accent');
    expect(parseCustomThemeJson('{ nope')).toEqual({ error: '无法解析 JSON，请检查引号、逗号和括号' });
  });
});
