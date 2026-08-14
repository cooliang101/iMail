import { describe, expect, it, vi } from 'vitest';
import type { Message } from '../../types';
import { applyOptimisticMessageMutation, MessageActionCoordinator, rollbackOptimisticMessageMutation } from './message-actions';

const message = { id: 'm1', unread: true, flagged: false } as Message;

describe('message action reliability', () => {
  it('applies and rolls back a failed optimistic mutation', () => {
    const optimistic = applyOptimisticMessageMutation([message], 'm1', { unread: false });
    expect(optimistic[0].unread).toBe(false);
    expect(rollbackOptimisticMessageMutation(optimistic, 'm1', { unread: false }, { unread: true })[0].unread).toBe(true);
  });

  it('does not overwrite a newer sync result while rolling back', () => {
    const synchronized = [{ ...message, unread: false, flagged: true }];
    const rolledBack = rollbackOptimisticMessageMutation(synchronized, 'm1', { unread: false, flagged: false }, { unread: true, flagged: false });
    expect(rolledBack[0]).toEqual(synchronized[0]);
  });

  it('rejects rapid duplicate actions for one message and releases after failure', async () => {
    const coordinator = new MessageActionCoordinator();
    let release!: () => void;
    const operation = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
    const first = coordinator.run('m1', operation);
    expect(await coordinator.run('m1', operation)).toBe(false);
    expect(operation).toHaveBeenCalledTimes(1);
    release();
    expect(await first).toBe(true);

    await expect(coordinator.run('m1', async () => { throw new Error('offline'); })).rejects.toThrow('offline');
    expect(await coordinator.run('m1', async () => undefined)).toBe(true);
  });
});
