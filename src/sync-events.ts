export type SyncEventType = 'connected' | 'sync.started' | 'sync.completed' | 'sync.failed' | 'message.created';

let eventSource: EventSource | undefined;
let subscriptionCount = 0;

function sharedEventSource() {
  eventSource ??= new EventSource('/api/events');
  return eventSource;
}

export function subscribeSyncEvents(types: SyncEventType[], listener: (event: MessageEvent) => void) {
  const source = sharedEventSource();
  const wrapped = (event: Event) => listener(event as MessageEvent);
  for (const type of types) source.addEventListener(type, wrapped);
  subscriptionCount += 1;

  return () => {
    for (const type of types) source.removeEventListener(type, wrapped);
    subscriptionCount -= 1;
    if (subscriptionCount === 0 && eventSource === source) {
      source.close();
      eventSource = undefined;
    }
  };
}
