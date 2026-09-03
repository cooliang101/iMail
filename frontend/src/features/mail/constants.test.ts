import { describe, expect, it } from 'vitest';
import { remainingMessageMoveUndoMs } from './constants';

describe('message move undo deadline', () => {
  it('keeps the original deadline when an effect is recreated', () => {
    expect(remainingMessageMoveUndoMs(5_500, 2_000)).toBe(3_500);
    expect(remainingMessageMoveUndoMs(5_500, 6_000)).toBe(0);
  });
});
