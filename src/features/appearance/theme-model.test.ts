import { describe, expect, it } from 'vitest';
import { defaultThemeId, normalizeThemeId } from './theme-model';

describe('theme model', () => {
  it('accepts every supported theme', () => {
    expect(normalizeThemeId('mint-fresh')).toBe('mint-fresh');
    expect(normalizeThemeId('tech')).toBe('tech');
    expect(normalizeThemeId('business-blue')).toBe('business-blue');
    expect(normalizeThemeId('soft-neubrutalism')).toBe('soft-neubrutalism');
  });

  it('falls back safely for removed or malformed themes', () => {
    expect(normalizeThemeId('imail-light')).toBe('mint-fresh');
    expect(normalizeThemeId(null)).toBe(defaultThemeId);
  });
});
