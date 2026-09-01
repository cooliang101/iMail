import { describe, expect, it } from 'vitest';
import { dateTimeLocalValue, nextHourLocalValue } from './schedule-send';

describe('scheduled send local time values', () => {
  it('formats local calendar fields instead of slicing UTC text', () => {
    const value = new Date(2026, 8, 1, 9, 7, 44);
    expect(dateTimeLocalValue(value)).toBe('2026-09-01T09:07');
  });

  it('defaults the picker to one hour from the supplied local time', () => {
    const value = new Date(2026, 8, 1, 23, 30, 12);
    expect(nextHourLocalValue(value)).toBe('2026-09-02T00:30');
  });
});
