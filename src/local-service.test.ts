import { describe, expect, it, vi } from 'vitest';
import type { DesktopHttpInvoker } from './desktop-http';
import { desktopEnableLocalService, desktopEnableLocalServiceAtPort, desktopLocalServiceStatus, desktopOpenAppLogs, desktopOpenLocalServiceLogs, desktopPauseLocalService, desktopRemoveLocalService } from './local-service';

describe('desktop local service bridge', () => {
  it('uses narrowly scoped Tauri commands for daemon lifecycle operations', async () => {
    const status = { installed: true, dataPresent: true, enabled: true, running: true, state: 'running', url: 'http://127.0.0.1:8787' };
    const invokeMock = vi.fn(async (_command: string) => status);
    const invoker = invokeMock as unknown as DesktopHttpInvoker;
    await expect(desktopLocalServiceStatus(invoker)).resolves.toEqual(status);
    await expect(desktopEnableLocalService(invoker)).resolves.toEqual(status);
    await expect(desktopEnableLocalServiceAtPort(18787, invoker)).resolves.toEqual(status);
    await expect(desktopPauseLocalService(invoker)).resolves.toEqual(status);
    await expect(desktopRemoveLocalService(invoker)).resolves.toEqual(status);
    await expect(desktopOpenLocalServiceLogs(invoker)).resolves.toEqual(status);
    await expect(desktopOpenAppLogs(invoker)).resolves.toEqual(status);
    expect(invokeMock.mock.calls).toEqual([
      ['local_service_status'],
      ['local_service_enable'],
      ['local_service_enable', { port: 18787 }],
      ['local_service_pause'],
      ['local_service_remove'],
      ['local_service_open_logs'],
      ['desktop_open_app_logs'],
    ]);
  });
});
