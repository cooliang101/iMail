import { describe, expect, it, vi } from 'vitest';
import { describeDesktopLogValue, desktopLog } from './logging';

describe('desktop logging bridge', () => {
  it('forwards bounded event data through the desktop command', async () => {
    const invoker = vi.fn().mockResolvedValue(undefined);
    await desktopLog('warn', 'service.connection_failed', 'request failed', true, invoker);
    expect(invoker).toHaveBeenCalledWith('desktop_log', {
      level: 'warn', event: 'service.connection_failed', message: 'request failed',
    });
  });

  it('does nothing in the web runtime', async () => {
    const invoker = vi.fn();
    await desktopLog('info', 'frontend.bootstrap', 'ready', false, invoker);
    expect(invoker).not.toHaveBeenCalled();
  });

  it('never turns a logging transport failure into an application error', async () => {
    const invoker = vi.fn().mockRejectedValue(new Error('bridge unavailable'));
    await expect(desktopLog('error', 'frontend.console_error', 'failed', true, invoker)).resolves.toBeUndefined();
  });

  it('does not serialize arbitrary objects into logs', () => {
    expect(describeDesktopLogValue({ password: 'must-not-appear' })).toBe('[Object]');
    expect(describeDesktopLogValue(new Error('failed'))).toContain('failed');
  });
});
