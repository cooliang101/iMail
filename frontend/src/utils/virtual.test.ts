import { describe, expect, it } from 'vitest';
import { virtualRange } from './virtual.js';

describe('virtualRange', () => {
  it('renders only the viewport plus overscan at the beginning', () => {
    expect(virtualRange(1_000, 0, 500, 100, 2)).toEqual({ start: 0, end: 7 });
  });

  it('moves the window as the list scrolls', () => {
    expect(virtualRange(1_000, 5_000, 500, 100, 3)).toEqual({ start: 47, end: 58 });
  });

  it('clamps the window at the end of the list', () => {
    expect(virtualRange(25, 2_300, 500, 100, 4)).toEqual({ start: 19, end: 25 });
  });

  it('handles empty and invalid dimensions safely', () => {
    expect(virtualRange(0, 0, 500, 100, 2)).toEqual({ start: 0, end: 0 });
    expect(virtualRange(10, 0, 500, 0, 2)).toEqual({ start: 0, end: 0 });
  });
});
