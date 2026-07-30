import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { MailAccount, StoreData, SyncPolicy } from '../types.js';
import type { SyncExecutionResult } from '../mail.js';
import { enqueueDueSyncs, targetsForPolicy } from './scheduler.js';
import { SyncStore } from './store.js';
import { executeSyncJob } from './worker-runtime.js';
import { startIdleWatchers } from './idle.js';
import { EventEmitter } from 'node:events';

const directories: string[] = [];
const stores: SyncStore[] = [];

async function temporarySyncStore() {
  const directory = await mkdtemp(path.join(tmpdir(), 'imail-sync-'));
  directories.push(directory);
  const store = new SyncStore(path.join(directory, 'imail.sqlite'));
  stores.push(store);
  return store;
}

function account(): MailAccount {
  return {
    id: '11111111-1111-4111-8111-111111111111', provider: 'gmail', email: 'owner@example.com', displayName: 'Owner', group: '个人', color: '#168f78',
    settings: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
    encryptedSecret: 'cipher', authMethod: 'oauth2', createdAt: '2026-07-30T00:00:00.000Z', status: 'connected',
  };
}

function data(): StoreData { return { accounts: [account()], messages: [], tokens: [], drafts: [], contacts: [], logoFetchAttempts: [] }; }

afterEach(async () => {
  while (stores.length) stores.pop()!.close();
  await Promise.all(directories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

describe('persistent synchronization control plane', () => {
  it('persists defaults and account-level policy overrides', async () => {
    const store = await temporarySyncStore();
    expect(store.getDefaultPolicy()).toMatchObject({ enabled: true, intervalMinutes: 5, folderMode: 'inbox' });
    store.updateDefaultPolicy({ intervalMinutes: 15, folderMode: 'standard' });
    expect(store.ensurePolicy(account().id)).toMatchObject({ intervalMinutes: 15, folderMode: 'standard' });
    expect(store.updatePolicy(account().id, { enabled: false })).toMatchObject({ enabled: false, intervalMinutes: 15 });
  });

  it('deduplicates active jobs and reclaims an expired lease', async () => {
    const store = await temporarySyncStore();
    const first = store.enqueueJob({ accountId: account().id, reason: 'manual', priority: 100 });
    expect(store.enqueueJob({ accountId: account().id, reason: 'manual', priority: 100 }).id).toBe(first.id);
    const base = new Date();
    const claimed = store.claimNextJob('worker-a', 10_000, base)!;
    expect(claimed).toMatchObject({ id: first.id, status: 'running', lockedBy: 'worker-a', attempts: 1 });
    expect(store.claimNextJob('worker-b', 10_000, new Date(base.getTime() + 5_000))).toBeUndefined();
    const reclaimed = store.claimNextJob('worker-b', 10_000, new Date(base.getTime() + 11_000));
    expect(reclaimed).toMatchObject({ id: first.id, lockedBy: 'worker-b', attempts: 2 });
    expect(() => store.completeJob(claimed, { mailbox: 'INBOX', lastSeenUid: 1, synced: 0, created: 0, updated: 0, deleted: 0 }, 5)).toThrow('同步任务租约已失效');
  });

  it('records successful cursor advancement and observable events', async () => {
    const store = await temporarySyncStore();
    const job = store.claimNextJob('worker-a', 60_000, new Date(),) ?? store.enqueueJob({ accountId: account().id, reason: 'manual' });
    const claimed = job.status === 'running' ? job : store.claimNextJob('worker-a', 60_000)!;
    store.markJobStarted(claimed, 'INBOX');
    store.completeJob(claimed, { mailbox: 'INBOX', uidValidity: '44', lastSeenUid: 102, synced: 2, created: 2, updated: 0, deleted: 0 }, 5);
    expect(store.getJob(claimed.id)).toMatchObject({ status: 'succeeded', syncedCount: 2, newCount: 2 });
    expect(store.getMailboxState(account().id, 'INBOX')).toMatchObject({ uidValidity: '44', lastSeenUid: 102, consecutiveFailures: 0, syncState: 'idle' });
    expect(store.listEvents(0).map((event) => event.type)).toEqual(['sync.started', 'sync.completed']);
  });

  it('schedules due work without a frontend connection and executes it through the worker task boundary', async () => {
    const store = await temporarySyncStore();
    const loadStore = vi.fn(async () => data());
    const jobs = await enqueueDueSyncs('scheduled', store, new Date('2026-07-30T02:00:00.000Z'), loadStore);
    expect(jobs).toHaveLength(1);
    const job = store.claimNextJob('worker-a', 60_000, new Date('2026-07-30T02:00:01.000Z'))!;
    const result: SyncExecutionResult = {
      synced: 1, created: 1, updated: 0, deleted: 0, mailbox: 'INBOX', mailboxRole: 'inbox', uidValidity: '8', lastSeenUid: 9, createdMessages: [],
    };
    const runSync = vi.fn(async () => result);
    await executeSyncJob(job, 'worker-a', store, 60_000, { loadStore, runSync });
    expect(runSync).toHaveBeenCalledOnce();
    expect(store.getJob(job.id)).toMatchObject({ status: 'succeeded', syncedCount: 1 });
  });

  it('expands standard and selected folder policies into explicit task targets', async () => {
    const configured = account(); configured.mailboxes = [{ path: 'Projects', name: 'Projects', delimiter: '/', selectable: true, subscribed: true }];
    const base: Omit<SyncPolicy, 'folderMode'> = { accountId: configured.id, enabled: true, intervalMinutes: 5, selectedMailboxes: [], syncOnStart: true, retryOnRecovery: true, notifyOnError: true, updatedAt: new Date().toISOString() };
    expect(targetsForPolicy(configured, { ...base, folderMode: 'standard' })).toEqual([{ mailboxRole: 'inbox' }, { mailboxRole: 'sent' }, { mailboxRole: 'archive' }]);
    expect(targetsForPolicy(configured, { ...base, folderMode: 'selected', selectedMailboxes: ['Projects', 'Missing'] })).toEqual([{ mailboxRole: 'inbox' }, { mailbox: 'Projects', mailboxRole: 'custom' }]);
  });

  it('pauses authentication failures instead of retrying forever', async () => {
    const store = await temporarySyncStore(); store.ensurePolicy(account().id);
    const queued = store.enqueueJob({ accountId: account().id, reason: 'manual' });
    const job = store.claimNextJob('worker-a', 60_000)!;
    await executeSyncJob(job, 'worker-a', store, 60_000, {
      loadStore: async () => data(),
      runSync: vi.fn(async () => { throw new Error('AUTHENTICATIONFAILED authorization=Bearer super-secret-token'); }),
    });
    expect(store.getJob(queued.id)).toMatchObject({ status: 'failed', errorCode: 'AUTH_REQUIRED' });
    expect(store.getJob(queued.id)?.errorMessage).not.toContain('super-secret-token');
    expect(store.getMailboxState(account().id, 'INBOX')).toMatchObject({ connectionStatus: 'authRequired', syncState: 'paused', nextSyncAt: undefined });
  });

  it('backs off transient failures and schedules recovery only when the retry is due', async () => {
    const store = await temporarySyncStore(); store.ensurePolicy(account().id);
    store.enqueueJob({ accountId: account().id, reason: 'scheduled' });
    const job = store.claimNextJob('worker-a', 60_000)!;
    await executeSyncJob(job, 'worker-a', store, 60_000, { loadStore: async () => data(), runSync: vi.fn(async () => { throw new Error('socket timeout'); }) });
    const state = store.getMailboxState(account().id, 'INBOX')!;
    expect(state).toMatchObject({ connectionStatus: 'unreachable', syncState: 'backoff', consecutiveFailures: 1 });
    expect((await enqueueDueSyncs('scheduled', store, new Date(new Date(state.nextSyncAt!).getTime() - 1), async () => data()))).toHaveLength(0);
    const recovery = await enqueueDueSyncs('scheduled', store, new Date(new Date(state.nextSyncAt!).getTime() + 1), async () => data());
    expect(recovery).toEqual([expect.objectContaining({ reason: 'recovery', status: 'queued' })]);
  });

  it('cancels an already claimed scheduled job when automatic synchronization is paused', async () => {
    const store = await temporarySyncStore(); store.ensurePolicy(account().id);
    const queued = store.enqueueJob({ accountId: account().id, reason: 'scheduled' });
    const job = store.claimNextJob('worker-a', 60_000)!;
    store.updatePolicy(account().id, { enabled: false });
    const runSync = vi.fn();
    await executeSyncJob(job, 'worker-a', store, 60_000, { loadStore: async () => data(), runSync });
    expect(runSync).not.toHaveBeenCalled();
    expect(store.getJob(queued.id)).toMatchObject({ status: 'cancelled' });
    expect(store.getMailboxState(account().id, 'INBOX')).toMatchObject({ syncState: 'paused', connectionStatus: 'connected' });
  });

  it('uses an IMAP IDLE watcher only as a persistent-task wake-up signal', async () => {
    const store = await temporarySyncStore(); store.ensurePolicy(account().id);
    const client = new EventEmitter() as EventEmitter & { connect: ReturnType<typeof vi.fn>; mailboxOpen: ReturnType<typeof vi.fn>; logout: ReturnType<typeof vi.fn> };
    client.connect = vi.fn(async () => undefined); client.mailboxOpen = vi.fn(async () => undefined); client.logout = vi.fn(async () => undefined);
    const watchers = startIdleWatchers({
      syncStore: store, autoStart: false, reconcileIntervalMs: 60_000, loadStore: async () => data(),
      createClient: vi.fn(async () => client as never),
    });
    await watchers.reconcile();
    expect(client.mailboxOpen).toHaveBeenCalledWith('INBOX', { readOnly: true });
    client.emit('exists', { count: 2, prevCount: 1 });
    expect(store.listJobs({ accountId: account().id })).toEqual([expect.objectContaining({ reason: 'recovery', status: 'queued', mailboxRole: 'inbox' })]);
    await watchers.close();
    expect(client.logout).toHaveBeenCalledOnce();
  });
});
