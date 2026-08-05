import type { ImapFlow } from 'imapflow';
import { imapClientFor } from '../mail/client.js';
import { readAllStore } from '../store.js';
import { getSyncStore, type SyncStore } from './store.js';

type Watcher = { client?: ImapFlow; closing: boolean; stableTimer?: NodeJS.Timeout; idleTask?: Promise<void> };

export function startIdleWatchers(options: { syncStore?: SyncStore; reconcileIntervalMs?: number; loadStore?: typeof readAllStore; createClient?: typeof imapClientFor; autoStart?: boolean } = {}) {
  const syncStore = options.syncStore ?? getSyncStore();
  const reconcileIntervalMs = Math.max(5_000, options.reconcileIntervalMs ?? Number(process.env.IMAIL_SYNC_IDLE_RECONCILE_MS ?? 5_000));
  const idleRefreshMs = Math.max(15_000, Number(process.env.IMAIL_SYNC_IDLE_REFRESH_MS ?? 60_000));
  const enabled = process.env.IMAIL_SYNC_IDLE_ENABLED !== 'false';
  const loadStore = options.loadStore ?? readAllStore;
  const createClient = options.createClient ?? imapClientFor;
  const watchers = new Map<string, Watcher>();
  const reconnectAttempts = new Map<string, number>();
  const reconnectTimers = new Map<string, NodeJS.Timeout>();
  let closed = false;
  let reconciling = false;

  const scheduleReconnect = (accountId: string) => {
    if (closed || reconnectTimers.has(accountId)) return;
    const attempt = (reconnectAttempts.get(accountId) ?? 0) + 1;
    reconnectAttempts.set(accountId, attempt);
    const delay = Math.min(30_000, 500 * 2 ** Math.min(attempt - 1, 6));
    const timer = setTimeout(() => {
      reconnectTimers.delete(accountId);
      void reconcile();
    }, delay);
    timer.unref(); reconnectTimers.set(accountId, timer);
  };

  const disconnect = (accountId: string, watcher: Watcher, error?: unknown) => {
    if (watcher.closing || watchers.get(accountId) !== watcher) return;
    watcher.closing = true; watchers.delete(accountId); clearTimeout(watcher.stableTimer);
    if (error) console.error(`[sync-idle] ${accountId}: ${error instanceof Error ? error.message : String(error)}`);
    watcher.client?.close();
    scheduleReconnect(accountId);
  };

  const stop = async (accountId: string) => {
    const watcher = watchers.get(accountId);
    if (!watcher) return;
    watcher.closing = true; watchers.delete(accountId); clearTimeout(watcher.stableTimer);
    await watcher.client?.logout().catch(() => undefined);
  };

  const connect = async (account: Awaited<ReturnType<typeof readAllStore>>['accounts'][number]) => {
    if (watchers.has(account.id) || closed) return;
    const watcher: Watcher = { closing: false }; watchers.set(account.id, watcher);
    const isCurrent = () => !closed && !watcher.closing && watchers.get(account.id) === watcher;
    try {
      const client = await createClient(account, { disableAutoIdle: true, maxIdleTime: idleRefreshMs, missingIdleCommand: 'STATUS' }); watcher.client = client;
      if (!isCurrent()) { await client.logout().catch(() => undefined); return; }
      const wake = (attempt = 0) => {
        if (!isCurrent()) return;
        try { syncStore.enqueueJob({ accountId: account.id, mailboxRole: 'inbox', reason: 'recovery', priority: 50 }); }
        catch (error) {
          if (attempt >= 4) {
            console.error(`[sync-idle] ${account.email}: 无法持久化邮箱唤醒事件`, error instanceof Error ? error.message : error);
            return;
          }
          const retryTimer = setTimeout(() => wake(attempt + 1), 250 * 2 ** attempt);
          retryTimer.unref();
        }
      };
      client.on('exists', () => wake()); client.on('flags', () => wake()); client.on('expunge', () => wake());
      client.on('error', (error) => disconnect(account.id, watcher, error));
      client.on('close', () => disconnect(account.id, watcher));
      await client.connect();
      if (!isCurrent()) { await client.logout().catch(() => undefined); return; }
      await client.mailboxOpen('INBOX', { readOnly: true });
      if (!isCurrent()) { await client.logout().catch(() => undefined); return; }
      // IDLE only signals that the mailbox may have changed. Reconcile once on
      // every successful (re)connection to cover events missed while offline.
      wake();
      watcher.stableTimer = setTimeout(() => reconnectAttempts.delete(account.id), 30_000);
      watcher.stableTimer.unref();
      watcher.idleTask = client.idle().then((result) => {
        if (!watcher.closing) disconnect(account.id, watcher, result === false ? new Error('IMAP IDLE 启动失败') : new Error('IMAP IDLE 意外结束'));
      }).catch((error) => disconnect(account.id, watcher, error));
    } catch (error) {
      if (watchers.get(account.id) === watcher) watchers.delete(account.id);
      watcher.closing = true; clearTimeout(watcher.stableTimer);
      await watcher.client?.logout().catch(() => undefined);
      const message = error instanceof Error ? error.message : 'IDLE 连接失败';
      console.error(`[sync-idle] ${account.email}: ${message}`);
      scheduleReconnect(account.id);
    }
  };

  async function reconcile() {
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
      await Promise.all(accounts.filter((account) => desired.has(account.id) && !watchers.has(account.id) && !reconnectTimers.has(account.id)).map(connect));
    } finally { reconciling = false; }
  }

  if (enabled && options.autoStart !== false) void reconcile();
  const timer = setInterval(() => { void reconcile(); }, reconcileIntervalMs);
  timer.unref();
  return {
    reconcile,
    async close() {
      closed = true; clearInterval(timer);
      for (const reconnectTimer of reconnectTimers.values()) clearTimeout(reconnectTimer);
      reconnectTimers.clear();
      await Promise.all([...watchers.keys()].map(stop));
    },
  };
}
