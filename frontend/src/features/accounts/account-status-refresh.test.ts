import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, subscribeSyncEvents } from '../../services';
import type { Account } from '../../types';
import { subscribeAccountStatus } from './account-status-refresh';

vi.mock('../../services', () => ({
  api: vi.fn(),
  accountsFromResponse: (value: { accounts: Account[] }) => value.accounts,
  subscribeSyncEvents: vi.fn(),
}));

let emit: () => void;
let stop: () => void;
const unsubscribe = vi.fn();
const failed = [{ id: 'mailbox', status: 'error', lastError: 'authentication failed' }];
const connected = [{ id: 'mailbox', status: 'connected' }];

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal('window', Object.assign(new EventTarget(), {
    setInterval: globalThis.setInterval, clearInterval: globalThis.clearInterval,
  }));
  vi.mocked(subscribeSyncEvents).mockImplementation((_types, listener) => {
    emit = () => listener({} as MessageEvent);
    return unsubscribe;
  });
});

afterEach(() => {
  stop?.();
  vi.clearAllMocks();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('account connection status refresh', () => {
  it('reflects sync failures and recovery without reopening settings', async () => {
    vi.mocked(api).mockResolvedValueOnce({ accounts: failed }).mockResolvedValueOnce({ accounts: connected });
    const update = vi.fn();
    stop = subscribeAccountStatus(update);
    expect(subscribeSyncEvents).toHaveBeenCalledWith(['connected', 'sync.completed', 'sync.failed'], expect.any(Function));
    emit();
    await vi.advanceTimersByTimeAsync(0);
    expect(update).toHaveBeenLastCalledWith(failed);
    emit();
    await vi.advanceTimersByTimeAsync(0);
    expect(update).toHaveBeenLastCalledWith(connected);
  });

  it('retries after a failed read and catches missed events through polling', async () => {
    vi.mocked(api).mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce({ accounts: failed });
    const update = vi.fn();
    stop = subscribeAccountStatus(update);
    window.dispatchEvent(new Event('focus'));
    await vi.advanceTimersByTimeAsync(0);
    expect(update).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(update).toHaveBeenCalledWith(failed);
  });

  it('rechecks events received during a read and ignores responses after disposal', async () => {
    let resolve!: (value: unknown) => void;
    vi.mocked(api).mockImplementationOnce(() => new Promise((done) => { resolve = done; }));
    vi.mocked(api).mockResolvedValueOnce({ accounts: failed });
    const update = vi.fn();
    stop = subscribeAccountStatus(update);
    emit();
    emit();
    expect(api).toHaveBeenCalledTimes(1);
    resolve({ accounts: connected });
    await vi.advanceTimersByTimeAsync(0);
    expect(api).toHaveBeenCalledTimes(2);
    expect(update).toHaveBeenLastCalledWith(failed);

    vi.mocked(api).mockImplementationOnce(() => new Promise((done) => { resolve = done; }));
    emit();
    stop();
    update.mockClear();
    resolve({ accounts: connected });
    await vi.advanceTimersByTimeAsync(30_000);
    expect(update).not.toHaveBeenCalled();
    expect(api).toHaveBeenCalledTimes(3);
  });
});
