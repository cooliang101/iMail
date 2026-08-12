import { describe, expect, it, vi } from 'vitest';
import type { ServiceInfo } from '../../types';
import { runWithReadySelectedService, switchToLocalService, switchToRemoteService, type ServiceTransitionDependencies } from './service-transition';

const info: ServiceInfo = {
  service: 'imail',
  instanceId: '11111111-1111-4111-8111-111111111111',
  version: '0.1.0',
  protocolVersion: 1,
  capabilities: { gateway: true, mcp: true, syncWorker: true, webClient: false },
};

function dependencies(overrides: Partial<ServiceTransitionDependencies> = {}): ServiceTransitionDependencies {
  return {
    testConnection: vi.fn(async () => info),
    saveSelection: vi.fn(),
    ...overrides,
  };
}

describe('service mode transition', () => {
  it('checks the embedded Rust identity before loading the local session', async () => {
    const order: string[] = [];
    await runWithReadySelectedService({
      mode: 'local',
      serviceUrl: 'imail://embedded',
      testConnection: vi.fn(async () => { order.push('identity'); return info; }),
    }, async () => { order.push('auth'); });
    expect(order).toEqual(['identity', 'auth']);
  });

  it('revalidates a saved remote identity before the first authenticated request', async () => {
    const order: string[] = [];
    await runWithReadySelectedService({
      mode: 'remote',
      serviceUrl: 'https://mail.example.com',
      testConnection: vi.fn(async () => { order.push('identity'); return info; }),
    }, async () => { order.push('auth'); });
    expect(order).toEqual(['identity', 'auth']);
  });

  it('rejects an insecure remote endpoint before any request', async () => {
    const testConnection = vi.fn(async () => info);
    await expect(runWithReadySelectedService({
      mode: 'remote',
      serviceUrl: 'http://192.168.1.20:8787',
      testConnection,
    }, async () => undefined)).rejects.toThrow('HTTPS');
    expect(testConnection).not.toHaveBeenCalled();
  });

  it('validates remote before saving and leaves selection unchanged on failure', async () => {
    const deps = dependencies({ testConnection: vi.fn(async () => { throw new Error('offline'); }) });
    await expect(switchToRemoteService('https://mail.example.com', deps)).rejects.toThrow('offline');
    expect(deps.saveSelection).not.toHaveBeenCalled();
  });

  it('normalizes and saves a verified remote endpoint', async () => {
    const deps = dependencies();
    await expect(switchToRemoteService('https://mail.example.com/', deps)).resolves.toBe(info);
    expect(deps.testConnection).toHaveBeenCalledWith('https://mail.example.com');
    expect(deps.saveSelection).toHaveBeenCalledWith({ mode: 'remote', remoteUrl: 'https://mail.example.com' });
  });

  it('switches to embedded local without daemon lifecycle operations', async () => {
    const deps = dependencies();
    await expect(switchToLocalService('imail://embedded', deps)).resolves.toBe(info);
    expect(deps.testConnection).toHaveBeenCalledWith('imail://embedded');
    expect(deps.saveSelection).toHaveBeenCalledWith({ mode: 'local' });
  });
});
