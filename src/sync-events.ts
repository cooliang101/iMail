export type SyncEventType = 'connected' | 'sync.status' | 'sync.started' | 'sync.completed' | 'sync.failed' | 'message.created';

import { configuredServiceUrl, serviceUrl } from './service-config';
import { isTauriRuntime } from './platform/tauri-runtime';

const eventTypes: SyncEventType[] = ['connected', 'sync.status', 'sync.started', 'sync.completed', 'sync.failed', 'message.created'];

type EventConnection = { close(): void };

let eventConnection: EventConnection | undefined;
let subscriptionCount = 0;
const listeners = new Map<SyncEventType, Set<(event: MessageEvent) => void>>();
const latestEvents = new Map<SyncEventType, MessageEvent>();

function dispatch(type: SyncEventType, data: string) {
  const message = new MessageEvent(type, { data });
  if (type === 'sync.status') latestEvents.set(type, message);
  for (const listener of listeners.get(type) ?? []) listener(message);
}

function webEventConnection(): EventConnection {
  const source = new EventSource(serviceUrl('/api/events'), { withCredentials: true });
  for (const type of eventTypes) source.addEventListener(type, (event) => dispatch(type, (event as MessageEvent).data));
  return { close: () => source.close() };
}

function desktopEventConnection(): EventConnection {
  let closed = false;
  let unlisten: (() => void) | undefined;
  void (async () => {
    const [{ invoke }, { listen }] = await Promise.all([import('@tauri-apps/api/core'), import('@tauri-apps/api/event')]);
    unlisten = await listen<{ event: SyncEventType; data: string }>('imail-sync-event', ({ payload }) => {
      if (!closed && eventTypes.includes(payload.event)) dispatch(payload.event, payload.data);
    });
    if (closed) { unlisten(); return; }
    await invoke('desktop_start_events', { baseUrl: configuredServiceUrl() });
  })().catch(() => undefined);
  return { close() {
    closed = true;
    unlisten?.();
    void import('@tauri-apps/api/core').then(({ invoke }) => invoke('desktop_stop_events')).catch(() => undefined);
  } };
}

function sharedEventConnection() {
  if (!eventConnection) {
    eventConnection = isTauriRuntime() ? desktopEventConnection() : webEventConnection();
  }
  return eventConnection;
}

export function subscribeSyncEvents(types: SyncEventType[], listener: (event: MessageEvent) => void) {
  const connection = sharedEventConnection();
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
    if (subscriptionCount === 0 && eventConnection === connection) {
      connection.close();
      eventConnection = undefined;
      latestEvents.clear();
    }
  };
}
