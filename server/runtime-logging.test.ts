import { describe, expect, it } from 'vitest';
import { describeRuntimeError, sanitizeRuntimeLogMessage } from './runtime-logging.js';

describe('runtime logging', () => {
  it('redacts credentials, OAuth parameters, and email addresses', () => {
    const output = sanitizeRuntimeLogMessage('alice@example.com Cookie: session=first; refresh=second\n{\"password\":\"hello\"} Bearer abc.def /?code=oauth-code&safe=yes');
    expect(output).not.toContain('alice@example.com');
    expect(output).not.toContain('hello');
    expect(output).not.toContain('abc.def');
    expect(output).not.toContain('oauth-code');
    expect(output).not.toContain('session=first');
    expect(output).not.toContain('refresh=second');
    expect(output).toContain('safe=yes');
  });

  it('does not serialize arbitrary rejected objects', () => {
    expect(describeRuntimeError({ refresh_token: 'must-not-appear' })).toBe('[Object]');
  });
});
