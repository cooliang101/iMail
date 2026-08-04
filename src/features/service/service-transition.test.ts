import { describe, expect, it, vi } from 'vitest';
import type { ServiceInfo } from '../../types';
import type { LocalServiceStatus } from '../../local-service';
import { runWithReadySelectedService, suspendManagedLocalService, switchToLocalService, switchToRemoteService, type ServiceTransitionDependencies } from './service-transition';

const info: ServiceInfo = {
  service: 'imail',
  instanceId: '11111111-1111-4111-8111-111111111111',
  version: '0.1.0',
  protocolVersion: 1,
  capabilities: { gateway: true, mcp: true, syncWorker: true, webClient: false },
};

const running: LocalServiceStatus = {
  installed: true,
  dataPresent: true,
  enabled: true,
  running: true,
  state: 'running',
  url: 'http://127.0.0.1:8787',
};

const stopped: LocalServiceStatus = {
  ...running,
  enabled: false,
  running: false,
  state: 'stopped',
};

function dependencies(overrides: Partial<ServiceTransitionDependencies> = {}): ServiceTransitionDependencies {
  return {
    desktop: true,
    currentMode: () => 'local',
    testConnection: vi.fn(async () => info),
    enableLocal: vi.fn(async () => running),
    pauseLocal: vi.fn(async () => stopped),
    saveSelection: vi.fn(),
    ...overrides,
  };
}

describe('service mode transition', () => {
  it('revalidates a saved remote identity before the first authenticated request', async () => {
    const order: string[] = [];
    const result = await runWithReadySelectedService({
      desktop: true,
      mode: 'remote',
      serviceUrl: 'https://mail.example.com',
      localSuspended: false,
      enableLocal: vi.fn(async () => running),
      testConnection: vi.fn(async () => { order.push('identity'); return info; }),
    }, async () => { order.push('auth'); return 'ready'; });
    expect(result).toBe('ready');
    expect(order).toEqual(['identity', 'auth']);
  });

  it('starts local, verifies its identity, and only then loads the session', async () => {
    const order: string[] = [];
    await runWithReadySelectedService({
      desktop: true,
      mode: 'local',
      serviceUrl: running.url,
      localSuspended: false,
      enableLocal: vi.fn(async () => { order.push('enable'); return running; }),
      testConnection: vi.fn(async () => { order.push('identity'); return info; }),
    }, async () => { order.push('auth'); });
    expect(order).toEqual(['enable', 'identity', 'auth']);
  });

  it('does not silently restart a local daemon explicitly paused by the user', async () => {
    const enableLocal = vi.fn(async () => running);
    const identity = vi.fn(async () => info);
    const auth = vi.fn(async () => undefined);
    await expect(runWithReadySelectedService({
      desktop: true,
      mode: 'local',
      serviceUrl: running.url,
      localSuspended: true,
      enableLocal,
      testConnection: identity,
    }, auth)).rejects.toThrow('本地服务已暂停或移除');
    expect(enableLocal).not.toHaveBeenCalled();
    expect(identity).not.toHaveBeenCalled();
    expect(auth).not.toHaveBeenCalled();
  });

  it('does not send a session request to an incompatible saved endpoint', async () => {
    const auth = vi.fn(async () => undefined);
    await expect(runWithReadySelectedService({
      desktop: true,
      mode: 'remote',
      serviceUrl: 'https://mail.example.com',
      localSuspended: false,
      enableLocal: vi.fn(async () => running),
      testConnection: vi.fn(async () => { throw new Error('protocol mismatch'); }),
    }, auth)).rejects.toThrow('protocol mismatch');
    expect(auth).not.toHaveBeenCalled();
  });

  it('rejects a saved non-loopback HTTP endpoint before identity or session requests', async () => {
    const identity = vi.fn(async () => info);
    const auth = vi.fn(async () => undefined);
    await expect(runWithReadySelectedService({
      desktop: true,
      mode: 'remote',
      serviceUrl: 'http://192.168.1.20:8787',
      localSuspended: false,
      enableLocal: vi.fn(async () => running),
      testConnection: identity,
    }, auth)).rejects.toThrow('HTTPS');
    expect(identity).not.toHaveBeenCalled();
    expect(auth).not.toHaveBeenCalled();
  });

  it('persists an explicit pause after the daemon has stopped', async () => {
    const order: string[] = [];
    const result = await suspendManagedLocalService({
      suspendLocal: vi.fn(async () => { order.push('pause'); return stopped; }),
      enableLocal: vi.fn(async () => { order.push('enable'); return running; }),
      saveSuspended: vi.fn(() => { order.push('save'); }),
    });
    expect(result).toBe(stopped);
    expect(order).toEqual(['pause', 'save']);
  });

  it('restores the daemon if persisting the explicit pause fails', async () => {
    const enableLocal = vi.fn(async () => running);
    await expect(suspendManagedLocalService({
      suspendLocal: vi.fn(async () => stopped),
      enableLocal,
      saveSuspended: vi.fn(() => { throw new Error('storage full'); }),
    })).rejects.toThrow('storage full');
    expect(enableLocal).toHaveBeenCalledOnce();
  });

  it('does not record a pause when the daemon did not stop', async () => {
    const saveSuspended = vi.fn();
    const enableLocal = vi.fn(async () => running);
    await expect(suspendManagedLocalService({
      suspendLocal: vi.fn(async () => { throw new Error('stop timeout'); }),
      enableLocal,
      saveSuspended,
    })).rejects.toThrow('stop timeout');
    expect(saveSuspended).not.toHaveBeenCalled();
    expect(enableLocal).not.toHaveBeenCalled();
  });

  it('reports both persistence and recovery failures while pausing', async () => {
    await expect(suspendManagedLocalService({
      suspendLocal: vi.fn(async () => stopped),
      enableLocal: vi.fn(async () => { throw new Error('restart failed'); }),
      saveSuspended: vi.fn(() => { throw new Error('storage full'); }),
    })).rejects.toThrow('storage full；恢复本地服务也失败：restart failed');
  });

  it('validates a remote service before pausing local and saving the selection', async () => {
    const order: string[] = [];
    const deps = dependencies({
      testConnection: vi.fn(async () => { order.push('test'); return info; }),
      pauseLocal: vi.fn(async () => { order.push('pause'); return { ...running, running: false }; }),
      saveSelection: vi.fn(() => { order.push('save'); }),
    });

    await switchToRemoteService('https://mail.example.com', deps);
    expect(order).toEqual(['test', 'pause', 'save']);
  });

  it('rejects non-loopback HTTP before contacting or pausing either service', async () => {
    const deps = dependencies();
    await expect(switchToRemoteService('http://mail.example.com:8787', deps)).rejects.toThrow('HTTPS');
    expect(deps.testConnection).not.toHaveBeenCalled();
    expect(deps.pauseLocal).not.toHaveBeenCalled();
    expect(deps.saveSelection).not.toHaveBeenCalled();
  });

  it('allows loopback HTTP for development and HTTPS for private network hosts', async () => {
    const loopback = dependencies();
    await switchToRemoteService('http://localhost:18787/', loopback);
    expect(loopback.testConnection).toHaveBeenCalledWith('http://localhost:18787');
    const privateTls = dependencies();
    await switchToRemoteService('https://192.168.1.20:8787/', privateTls);
    expect(privateTls.testConnection).toHaveBeenCalledWith('https://192.168.1.20:8787');
  });

  it('keeps local running when remote validation fails', async () => {
    const deps = dependencies({ testConnection: vi.fn(async () => { throw new Error('offline'); }) });
    await expect(switchToRemoteService('https://mail.example.com', deps)).rejects.toThrow('offline');
    expect(deps.pauseLocal).not.toHaveBeenCalled();
    expect(deps.saveSelection).not.toHaveBeenCalled();
  });

  it('restores the previous local service when saving remote mode fails', async () => {
    const deps = dependencies({ saveSelection: vi.fn(() => { throw new Error('storage full'); }) });
    await expect(switchToRemoteService('https://mail.example.com', deps)).rejects.toThrow('storage full');
    expect(deps.enableLocal).toHaveBeenCalledOnce();
  });

  it('restores the previous local service when pause reports a partial failure', async () => {
    const deps = dependencies({ pauseLocal: vi.fn(async () => { throw new Error('stop timeout'); }) });
    await expect(switchToRemoteService('https://mail.example.com', deps)).rejects.toThrow('stop timeout');
    expect(deps.enableLocal).toHaveBeenCalledOnce();
    expect(deps.saveSelection).not.toHaveBeenCalled();
  });

  it('only saves local mode after the daemon and identity are ready', async () => {
    const order: string[] = [];
    const deps = dependencies({
      currentMode: () => 'remote',
      enableLocal: vi.fn(async () => { order.push('enable'); return running; }),
      testConnection: vi.fn(async () => { order.push('test'); return info; }),
      saveSelection: vi.fn(() => { order.push('save'); }),
    });
    await switchToLocalService(running.url, deps);
    expect(order).toEqual(['enable', 'test', 'save']);
  });

  it('validates and persists the port actually returned by a reconfigured daemon', async () => {
    const alternate = { ...running, url: 'http://127.0.0.1:18787' };
    const deps = dependencies({
      currentMode: () => 'remote',
      enableLocal: vi.fn(async () => alternate),
    });
    await switchToLocalService(running.url, deps, 18787);
    expect(deps.enableLocal).toHaveBeenCalledWith(18787);
    expect(deps.testConnection).toHaveBeenCalledWith(alternate.url);
    expect(deps.saveSelection).toHaveBeenCalledWith({ mode: 'local', localPort: 18787 });
  });

  it('pauses a newly enabled local daemon if switching from remote fails', async () => {
    const deps = dependencies({
      currentMode: () => 'remote',
      testConnection: vi.fn(async () => { throw new Error('wrong identity'); }),
    });
    await expect(switchToLocalService(running.url, deps)).rejects.toThrow('wrong identity');
    expect(deps.pauseLocal).toHaveBeenCalledOnce();
    expect(deps.saveSelection).not.toHaveBeenCalled();
  });

  it('restores remote mode even when enabling local reports a partial failure', async () => {
    const deps = dependencies({
      currentMode: () => 'remote',
      enableLocal: vi.fn(async () => { throw new Error('enable timeout'); }),
    });
    await expect(switchToLocalService(running.url, deps)).rejects.toThrow('enable timeout');
    expect(deps.pauseLocal).toHaveBeenCalledOnce();
    expect(deps.saveSelection).not.toHaveBeenCalled();
  });

  it('reports both the transition and rollback failures', async () => {
    const deps = dependencies({
      saveSelection: vi.fn(() => { throw new Error('storage full'); }),
      enableLocal: vi.fn(async () => { throw new Error('restart failed'); }),
    });
    await expect(switchToRemoteService('https://mail.example.com', deps))
      .rejects.toThrow('storage full；恢复原本地服务也失败：restart failed');
  });
});
