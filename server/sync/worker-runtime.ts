import { hostname } from 'node:os';
import { syncMailbox } from '../mail.js';
import { readAllStore } from '../store.js';
import type { MailboxSyncState, SyncJob } from '../types.js';
import { reconciliationIntervalMinutes, startScheduler } from './scheduler.js';
import { getSyncStore, type SyncStore } from './store.js';
import { startIdleWatchers } from './idle.js';

const retryScheduleMinutes = [1, 5, 15, 30, 60] as const;

function targetKey(job: SyncJob) {
  return job.mailbox ?? (job.mailboxRole === 'inbox' ? 'INBOX' : `@role:${job.mailboxRole}`);
}

function stateForJob(states: MailboxSyncState[], job: SyncJob) {
  if (job.mailbox) return states.find((state) => state.mailbox === job.mailbox);
  return states.find((state) => state.mailboxRole === job.mailboxRole && !state.mailbox.startsWith('@role:'))
    ?? states.find((state) => state.mailboxRole === job.mailboxRole);
}

function classifyFailure(error: unknown) {
  const raw = error instanceof Error ? error.message : '同步失败';
  const message = raw.replace(/Bearer\s+[^\s,;]+/gi, 'Bearer [redacted]')
    .replace(/(access[_-]?token|refresh[_-]?token|password|authorization)(\s*[:=]\s*)[^\s,;]+/gi, '$1$2[redacted]')
    .slice(0, 1_000);
  const authRequired = /auth|credential|password|token|oauth|登录|授权|凭据/i.test(message);
  const code = authRequired ? 'AUTH_REQUIRED' : /timeout|timed out|超时/i.test(message) ? 'TIMEOUT' : 'IMAP_UNAVAILABLE';
  return { code, message, authRequired };
}

export async function executeSyncJob(job: SyncJob, workerId: string, syncStore: SyncStore, leaseMs: number, dependencies: { loadStore?: typeof readAllStore; runSync?: typeof syncMailbox; reconciliationMinutes?: number } = {}) {
  const loadStore = dependencies.loadStore ?? readAllStore;
  const data = await loadStore();
  const account = data.accounts.find((item) => item.id === job.accountId);
  const mailboxKey = targetKey(job);
  if (!account) {
    syncStore.failJob(job, { mailbox: mailboxKey, code: 'ACCOUNT_NOT_FOUND', message: '邮箱账户不存在', authRequired: false });
    return;
  }
  const policy = syncStore.ensurePolicy(account.id);
  if (!policy.enabled && job.reason !== 'manual') {
    syncStore.cancelJob(job, mailboxKey);
    return;
  }
  const previous = stateForJob(syncStore.listMailboxStates(account.id), job);
  syncStore.markJobStarted(job, previous?.mailbox ?? mailboxKey);
  const heartbeat = setInterval(() => { syncStore.renewLease(job.id, workerId, leaseMs); }, Math.max(1_000, Math.floor(leaseMs / 3)));
  heartbeat.unref();
  try {
    const result = await (dependencies.runSync ?? syncMailbox)(account.id, job.mailboxRole, job.mailbox, previous && {
      uidValidity: previous.uidValidity, lastSeenUid: previous.lastSeenUid, highestModseq: previous.highestModseq,
    });
    if (!(await loadStore()).accounts.some((item) => item.id === account.id)) { syncStore.deleteAccountData(account.id); return; }
    syncStore.completeJob(job, result, dependencies.reconciliationMinutes ?? reconciliationIntervalMinutes());
    try { syncStore.recordMessageCreated(account, result.createdMessages); }
    catch (eventError) { console.error('[sync-worker] message event persistence failed', eventError instanceof Error ? eventError.message : eventError); }
  } catch (error) {
    if (!(await loadStore()).accounts.some((item) => item.id === account.id)) { syncStore.deleteAccountData(account.id); return; }
    const failure = classifyFailure(error);
    const failureCount = (previous?.consecutiveFailures ?? 0) + 1;
    const retryMinutes = failure.authRequired ? undefined : retryScheduleMinutes[Math.min(failureCount - 1, retryScheduleMinutes.length - 1)];
    syncStore.failJob(job, { mailbox: job.mailbox ?? previous?.mailbox ?? mailboxKey, ...failure }, retryMinutes);
  } finally { clearInterval(heartbeat); }
}

export function startSyncWorker(options: { syncStore?: SyncStore; pollIntervalMs?: number; leaseMs?: number; schedulerIntervalMs?: number; workerId?: string } = {}) {
  const syncStore = options.syncStore ?? getSyncStore();
  const hostName = hostname();
  const workerId = options.workerId ?? `${hostName}:${process.pid}:${crypto.randomUUID()}`;
  const startedAt = new Date().toISOString();
  const pollIntervalMs = Math.max(250, options.pollIntervalMs ?? Number(process.env.IMAIL_SYNC_WORKER_POLL_MS ?? 1_000));
  const leaseMs = Math.max(10_000, options.leaseMs ?? Number(process.env.IMAIL_SYNC_JOB_LEASE_MS ?? 120_000));
  const concurrency = Math.min(10, Math.max(1, Number(process.env.IMAIL_SYNC_CONCURRENCY ?? 3)));
  const scheduler = startScheduler({ intervalMs: options.schedulerIntervalMs, syncStore });
  const idleWatchers = startIdleWatchers({ syncStore });
  const active = new Set<Promise<void>>();
  let closed = false;

  const poll = () => {
    if (closed) return;
    while (active.size < concurrency) {
      const job = syncStore.claimNextJob(workerId, leaseMs);
      if (!job) break;
      const operation = executeSyncJob(job, workerId, syncStore, leaseMs)
        .catch((error) => console.error('[sync-worker] task failed', error instanceof Error ? error.message : error))
        .finally(() => active.delete(operation));
      active.add(operation);
    }
  };
  const timer = setInterval(poll, pollIntervalMs);
  syncStore.heartbeat(workerId, process.pid, hostName, startedAt);
  const heartbeatTimer = setInterval(() => syncStore.heartbeat(workerId, process.pid, hostName, startedAt), 10_000);
  heartbeatTimer.unref();
  poll();

  return {
    workerId,
    poll,
    async close() {
      closed = true; clearInterval(timer); clearInterval(heartbeatTimer); scheduler.close();
      await idleWatchers.close();
      await Promise.allSettled(active);
      syncStore.removeHeartbeat(workerId);
    },
  };
}
