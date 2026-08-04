import { describe, expect, it, vi } from 'vitest';
import { parentProcessAlive } from './worker.js';

describe('sync worker parent watchdog', () => {
  it('treats an existing parent and permission-protected parent as alive', () => {
    expect(parentProcessAlive(42, vi.fn())).toBe(true);
    expect(parentProcessAlive(42, vi.fn(() => { throw Object.assign(new Error('denied'), { code: 'EPERM' }); }))).toBe(true);
  });

  it('treats missing and invalid parent processes as dead', () => {
    expect(parentProcessAlive(42, vi.fn(() => { throw Object.assign(new Error('missing'), { code: 'ESRCH' }); }))).toBe(false);
    expect(parentProcessAlive(0, vi.fn())).toBe(false);
  });
});
