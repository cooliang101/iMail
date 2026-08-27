import { describe, expect, it } from 'vitest';
import { normalizeLanguage } from './i18n';

describe('i18n language normalization', () => {
  it('accepts the two supported locales', () => {
    expect(normalizeLanguage('zh-CN')).toBe('zh-CN');
    expect(normalizeLanguage('en-US')).toBe('en-US');
  });

  it('falls back safely to Simplified Chinese', () => {
    expect(normalizeLanguage('fr-FR')).toBe('zh-CN');
    expect(normalizeLanguage(undefined)).toBe('zh-CN');
  });
});
