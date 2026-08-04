import { describe, expect, it } from 'vitest';
import { daemonShutdownAuthorized } from './service-info.js';

describe('daemon shutdown authorization', () => {
  it('requires both loopback transport and an exact token', () => {
    expect(daemonShutdownAuthorized('127.0.0.1', 'secret', 'secret\n')).toBe(true);
    expect(daemonShutdownAuthorized('::1', 'secret', 'secret')).toBe(true);
    expect(daemonShutdownAuthorized('192.0.2.4', 'secret', 'secret')).toBe(false);
    expect(daemonShutdownAuthorized('127.0.0.1', 'wrong', 'secret')).toBe(false);
    expect(daemonShutdownAuthorized('127.0.0.1', undefined, 'secret')).toBe(false);
  });
});
