import { describe, expect, it, vi } from 'vitest';
import { applySettledResult } from './settled-result';

describe('settled response application', () => {
  it('returns one parse failure without preventing another result from applying', () => {
    const apply = vi.fn();
    const failure = applySettledResult({ status: 'fulfilled', value: null }, () => { throw new Error('invalid'); }, apply);
    const success = applySettledResult({ status: 'fulfilled', value: ['ok'] }, (value) => value as string[], apply);
    expect(failure).toBeInstanceOf(Error);
    expect(success).toBeUndefined();
    expect(apply).toHaveBeenCalledWith(['ok']);
  });
});
