import { afterEach, describe, expect, it, vi } from 'vitest';
import { subscribeSyncEvents } from './sync-events';

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  readonly listeners = new Map<string, Set<EventListenerOrEventListenerObject>>();
  closed = false;

  constructor(readonly url: string | URL) { FakeEventSource.instances.push(this); }

  addEventListener(type: string, listener: EventListenerOrEventListenerObject) {
    const listeners = this.listeners.get(type) ?? new Set();
    listeners.add(listener); this.listeners.set(type, listeners);
  }

  removeEventListener(type: string, listener: EventListenerOrEventListenerObject) {
    this.listeners.get(type)?.delete(listener);
  }

  close() { this.closed = true; }

  emit(type: string) {
    const event = { type } as MessageEvent;
    for (const listener of this.listeners.get(type) ?? []) {
      if (typeof listener === 'function') listener(event);
      else listener.handleEvent(event);
    }
  }
}

afterEach(() => {
  FakeEventSource.instances = [];
  vi.unstubAllGlobals();
});

describe('shared sync events', () => {
  it('shares one SSE connection and closes it after the last subscriber leaves', () => {
    vi.stubGlobal('EventSource', FakeEventSource);
    const completed = vi.fn();
    const created = vi.fn();
    const unsubscribeCompleted = subscribeSyncEvents(['sync.completed'], completed);
    const unsubscribeCreated = subscribeSyncEvents(['message.created'], created);

    expect(FakeEventSource.instances).toHaveLength(1);
    expect(FakeEventSource.instances[0].url).toBe('/api/events');
    FakeEventSource.instances[0].emit('sync.completed');
    expect(completed).toHaveBeenCalledOnce();
    expect(created).not.toHaveBeenCalled();

    unsubscribeCompleted();
    expect(FakeEventSource.instances[0].closed).toBe(false);
    unsubscribeCreated();
    expect(FakeEventSource.instances[0].closed).toBe(true);
  });
});
