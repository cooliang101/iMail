import { describe, expect, it, vi } from 'vitest';
import { createLatestServiceCheckRunner } from './latest-service-check';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, decline) => { resolve = accept; reject = decline; });
  return { promise, resolve, reject };
}

describe('latest service check runner', () => {
  it('ignores a previous service response that arrives after a newer selection', async () => {
    const runner = createLatestServiceCheckRunner();
    const first = deferred<string>();
    const second = deferred<string>();
    const accepted: string[] = [];
    const settled = vi.fn();
    const handlers = { onSuccess: (value: string) => accepted.push(value), onError: vi.fn(), onSettled: settled };
    const firstRun = runner.run(() => first.promise, handlers);
    const secondRun = runner.run(() => second.promise, handlers);
    second.resolve('remote-b');
    await secondRun;
    first.resolve('remote-a');
    await firstRun;
    expect(accepted).toEqual(['remote-b']);
    expect(settled).toHaveBeenCalledOnce();
  });

  it('suppresses stale errors and all callbacks after cancellation', async () => {
    const runner = createLatestServiceCheckRunner();
    const pending = deferred<string>();
    const handlers = { onSuccess: vi.fn(), onError: vi.fn(), onSettled: vi.fn() };
    const run = runner.run(() => pending.promise, handlers);
    runner.cancel();
    pending.reject(new Error('old endpoint failed'));
    await run;
    expect(handlers.onSuccess).not.toHaveBeenCalled();
    expect(handlers.onError).not.toHaveBeenCalled();
    expect(handlers.onSettled).not.toHaveBeenCalled();
  });
});
