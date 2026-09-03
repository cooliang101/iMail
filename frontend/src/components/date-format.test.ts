import { describe, expect, it, vi } from 'vitest';
import { formatDate, parseValidDate, relativeTime } from './date-format';

describe('date formatting', () => {
  it('uses safe fallbacks for missing and invalid values', () => {
    expect(parseValidDate(null)).toBeUndefined();
    expect(formatDate('not-a-date', { year: 'numeric' })).toBe('—');
    expect(relativeTime(undefined)).toBe('时间未知');
  });

  it('does not describe future timestamps as elapsed time', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-09-03T00:00:00Z'));
    expect(relativeTime('2026-09-04T00:00:00Z')).not.toContain('前');
    vi.useRealTimers();
  });
});
