import type { ImapFlow } from 'imapflow';
import { imapClientFor } from '../mail/client.js';
import { readAllStore } from '../store.js';
import { getSyncStore, type SyncStore } from './store.js';

type Watcher = { client?: ImapFlow; closing: boolean };

export function startIdleWatchers(options: { syncStore?: SyncStore; reconcileIntervalMs?: number; loadStore?: typeof readAllStore; createClient?: typeof imapClientFor; autoStart?: boolean } = {}) {
  const syncStore = options.syncStore ?? getSyncStore();
  const reconcileIntervalMs = Math.max(5_000, options.reconcileIntervalMs ?? Number(process.env.IMAIL_SYNC_IDLE_RECONCILE_MS ?? 30_000));
  const enabled = process.env.IMAIL_SYNC_IDLE_ENABLED !== 'false';
  const loadStore = options.loadStore ?? readAllStore;
  const createClient = options.createClient ?? imapClientFor;
  const watchers = new Map<string, Watcher>();
  let closed = false;
  let reconciling = false;

  const stop = async (accountId: string) => {
    const watcher = watchers.get(accountId);
    if (!watcher) return;
    watcher.closing = true; watchers.delete(accountId);
    await watcher.client?.logout().catch(() => undefined);
  };

  const connect = async (account: Awaited<ReturnType<typeof readAllStore>>['accounts'][number]) => {
    if (watchers.has(account.id) || closed) return;
    const watcher: Watcher = { closing: false }; watchers.set(account.id, watcher);
    try {
      const client = await createClient(account); watcher.client = client;
      const wake = () => {
        if (watcher.closing || closed) return;
        syncStore.enqueueJob({ accountId: account.id, mailboxRole: 'inbox', reason: 'recovery', priority: 50 });
      };
      client.on('exists', wake); client.on('flags', wake); client.on('expunge', wake);
      client.on('error', () => undefined);
      client.on('close', () => { if (!watcher.closing) watchers.delete(account.id); });
      await client.connect();
      await client.mailboxOpen('INBOX', { readOnly: true });
    } catch (error) {
      watchers.delete(account.id);
      await watcher.client?.logout().catch(() => undefined);
      const message = error instanceof Error ? error.message : 'IDLE 连接失败';
      console.error(`[sync-idle] ${account.email}: ${message}`);
    }
  };

  const reconcile = async () => {
    if (!enabled || closed || reconciling) return;
    reconciling = true;
    try {
      const { accounts } = await loadStore();
      const desired = new Set(accounts.filter((account) => {
        const policy = syncStore.ensurePolicy(account.id);
        const authRequired = syncStore.listMailboxStates(account.id).some((state) => state.connectionStatus === 'authRequired');
        return policy.enabled && !authRequired;
      }).map((account) => account.id));
      await Promise.all([...watchers.keys()].filter((accountId) => !desired.has(accountId)).map(stop));
      await Promise.all(accounts.filter((account) => desired.has(account.id) && !watchers.has(account.id)).map(connect));
    } finally { reconciling = false; }
  };

  if (enabled && options.autoStart !== false) void reconcile();
  const timer = setInterval(() => { void reconcile(); }, reconcileIntervalMs);
  timer.unref();
  return {
    reconcile,
    async close() { closed = true; clearInterval(timer); await Promise.all([...watchers.keys()].map(stop)); },
  };
}
