import { expect, it } from 'vitest';
import { desktopOpenAppLogs } from './app-logs';
import type { DesktopHttpInvoker } from './http';

it('opens only the desktop application logs', async () => {
  const calls: unknown[][] = [];
  const invoker: DesktopHttpInvoker = async <T>(...args: unknown[]) => {
    calls.push(args);
    return undefined as T;
  };
  await desktopOpenAppLogs(invoker);
  expect(calls).toEqual([['desktop_open_app_logs']]);
});
