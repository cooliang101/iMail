export type SyncEventType = 'connected' | 'sync.status' | 'sync.started' | 'sync.completed' | 'sync.failed' | 'message.created';

const eventTypes: SyncEventType[] = ['connected', 'sync.status', 'sync.started', 'sync.completed', 'sync.failed', 'message.created'];

let eventSource: EventSource | undefined;
let subscriptionCount = 0;
const listeners = new Map<SyncEventType, Set<(event: MessageEvent) => void>>();
const latestEvents = new Map<SyncEventType, MessageEvent>();

function sharedEventSource() {
  if (!eventSource) {
    eventSource = new EventSource('/api/events');
    for (const type of eventTypes) eventSource.addEventListener(type, (event) => {
      const message = event as MessageEvent;
      if (type === 'sync.status') latestEvents.set(type, message);
      for (const listener of listeners.get(type) ?? []) listener(message);
    });
  }
  return eventSource;
}

export function subscribeSyncEvents(types: SyncEventType[], listener: (event: MessageEvent) => void) {
  const source = sharedEventSource();
  for (const type of types) {
    const typeListeners = listeners.get(type) ?? new Set();
    typeListeners.add(listener);
    listeners.set(type, typeListeners);
  }
  subscriptionCount += 1;
  for (const type of types) {
    const latest = latestEvents.get(type);
    if (latest) queueMicrotask(() => {
      if (listeners.get(type)?.has(listener)) listener(latest);
    });
  }

  return () => {
    for (const type of types) listeners.get(type)?.delete(listener);
    subscriptionCount -= 1;
    if (subscriptionCount === 0 && eventSource === source) {
      source.close();
      eventSource = undefined;
      latestEvents.clear();
    }
  };
}
