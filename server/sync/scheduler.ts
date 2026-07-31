import type { MailAccount, MailboxRole, SyncJobReason, SyncPolicy } from '../types.js';
import { readAllStore } from '../store.js';
import { getSyncStore, type SyncStore } from './store.js';
import { canonicalSyncTarget } from '../mail/mailbox-role.js';

export type SyncTarget = { mailbox?: string; mailboxRole: MailboxRole };

export function targetsForPolicy(account: MailAccount, policy: SyncPolicy): SyncTarget[] {
  const targets: SyncTarget[] = [{ mailboxRole: 'inbox' }];
  if (policy.folderMode === 'standard') targets.push({ mailboxRole: 'sent' }, { mailboxRole: 'archive' });
  if (policy.folderMode === 'selected') {
    for (const mailbox of policy.selectedMailboxes) {
      const folder = account.mailboxes?.find((item) => item.path === mailbox && item.selectable);
      if (folder) {
        const target = canonicalSyncTarget(account, 'custom', folder.path);
        const key = target.mailbox ?? `@role:${target.mailboxRole}`;
        if (!targets.some((item) => (item.mailbox ?? `@role:${item.mailboxRole}`) === key)) targets.push(target);
      }
    }
  }
  return targets;
}

function stateForTarget(states: ReturnType<SyncStore['listMailboxStates']>, target: SyncTarget) {
  return target.mailbox
    ? states.find((state) => state.mailbox === target.mailbox)
    : states.find((state) => state.mailboxRole === target.mailboxRole && !state.mailbox.startsWith('@role:'))
      ?? states.find((state) => state.mailboxRole === target.mailboxRole);
}

export async function enqueueDueSyncs(reason: SyncJobReason = 'scheduled', syncStore = getSyncStore(), now = new Date(), loadStore = readAllStore) {
  const { accounts } = await loadStore();
  const nowIso = now.toISOString();
  const jobs = [];
  for (const account of accounts) {
    const policy = syncStore.ensurePolicy(account.id);
    if (!policy.enabled || (reason === 'startup' && !policy.syncOnStart)) continue;
    if (account.status === 'connected') syncStore.resumeAfterAuthorization(account.id);
    const states = syncStore.listMailboxStates(account.id);
    for (const target of targetsForPolicy(account, policy)) {
      const state = stateForTarget(states, target);
      if (state?.connectionStatus === 'authRequired' || state?.syncState === 'paused') continue;
      if (state?.nextSyncAt && state.nextSyncAt > nowIso) continue;
      jobs.push(syncStore.enqueueJob({ accountId: account.id, ...target, reason: state?.consecutiveFailures ? 'recovery' : reason, priority: reason === 'startup' ? 5 : 0, notBefore: nowIso }));
    }
  }
  syncStore.pruneEvents(new Date(now.getTime() - 7 * 24 * 60 * 60_000).toISOString());
  return jobs;
}

export function startScheduler(options: { intervalMs?: number; syncStore?: SyncStore } = {}) {
  const syncStore = options.syncStore ?? getSyncStore();
  const intervalMs = Math.max(5_000, options.intervalMs ?? Number(process.env.IMAIL_SYNC_SCHEDULER_INTERVAL_MS ?? 5_000));
  let running = false;
  const scan = async (reason: SyncJobReason = 'scheduled') => {
    if (running) return [];
    running = true;
    try { return await enqueueDueSyncs(reason, syncStore); }
    finally { running = false; }
  };
  const startupTimer = setTimeout(() => { void scan('startup'); }, Math.max(0, Number(process.env.IMAIL_SYNC_STARTUP_DELAY_MS ?? 1_000)));
  startupTimer.unref();
  const timer = setInterval(() => { void scan(); }, intervalMs);
  timer.unref();
  return {
    scan,
    close() { clearTimeout(startupTimer); clearInterval(timer); },
  };
}
