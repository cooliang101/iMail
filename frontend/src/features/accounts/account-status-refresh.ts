import { accountsFromResponse, api, subscribeSyncEvents } from '../../services';
import type { Account } from '../../types';

// Fetch the shared account read model: desktop, HTTP and MCP see the same state.
export function subscribeAccountStatus(onAccounts: (accounts: Account[]) => void) {
  let active = true;
  let running = false;
  let pending = false;
  const refresh = async () => {
    if (!active) return;
    pending = true;
    if (running) return;
    running = true;
    try {
      while (active && pending) {
        pending = false;
        try {
          const accounts = accountsFromResponse(await api<unknown>('/api/accounts'));
          if (active) onAccounts(accounts);
        } catch {
          // Keep the last known state; reconnect, focus and polling retry the read.
        }
      }
    } finally {
      running = false;
    }
  };
  const unsubscribe = subscribeSyncEvents(['connected', 'sync.completed', 'sync.failed'], () => { void refresh(); });
  const onFocus = () => { void refresh(); };
  window.addEventListener('focus', onFocus);
  const timer = window.setInterval(onFocus, 30_000);
  return () => {
    active = false;
    unsubscribe();
    window.removeEventListener('focus', onFocus);
    window.clearInterval(timer);
  };
}
