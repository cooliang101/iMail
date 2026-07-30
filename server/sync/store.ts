import crypto from 'node:crypto';
import path from 'node:path';
import { mkdirSync } from 'node:fs';
import { DatabaseSync } from 'node:sqlite';
import { ensureSchema } from '../storage/schema.js';
import { configuredDatabasePath, registerAuxiliaryStoreCloser } from '../store.js';
import type {
  CachedMessage, MailAccount, MailboxRole, MailboxSyncState, SyncConnectionStatus, SyncEvent, SyncFolderMode,
  SyncJob, SyncJobReason, SyncPolicy, SyncPolicySettings, SyncState,
} from '../types.js';
import { clientMessageSummary, integrationMessageSummary } from '../domain/message-views.js';
import { currentUserId } from '../auth/context.js';

type Row = Record<string, string | number | bigint | null>;

export const defaultSyncPolicy = (accountId: string, now = new Date().toISOString()): SyncPolicy => ({
  accountId,
  enabled: true,
  intervalMinutes: 5,
  folderMode: 'inbox',
  selectedMailboxes: [],
  syncOnStart: true,
  retryOnRecovery: true,
  notifyOnError: true,
  updatedAt: now,
});

const defaultSettings = (): SyncPolicySettings => {
  const { accountId: _accountId, updatedAt: _updatedAt, ...settings } = defaultSyncPolicy('*');
  return settings;
};

function optional(row: Row, key: string) {
  const value = row[key];
  return value === null || value === undefined ? undefined : String(value);
}

function policyFromRow(row: Row): SyncPolicy {
  return {
    accountId: String(row.account_id), enabled: Boolean(row.enabled), intervalMinutes: Number(row.interval_minutes),
    folderMode: String(row.folder_mode) as SyncFolderMode,
    selectedMailboxes: JSON.parse(String(row.selected_mailboxes_json)) as string[],
    syncOnStart: Boolean(row.sync_on_start), retryOnRecovery: Boolean(row.retry_on_recovery),
    notifyOnError: Boolean(row.notify_on_error), updatedAt: String(row.updated_at),
  };
}

function stateFromRow(row: Row): MailboxSyncState {
  return {
    accountId: String(row.account_id), mailbox: String(row.mailbox), mailboxRole: String(row.mailbox_role) as MailboxRole,
    uidValidity: optional(row, 'uid_validity'), lastSeenUid: Number(row.last_seen_uid), highestModseq: optional(row, 'highest_modseq'),
    lastAttemptAt: optional(row, 'last_attempt_at'), lastSuccessAt: optional(row, 'last_success_at'), nextSyncAt: optional(row, 'next_sync_at'),
    consecutiveFailures: Number(row.consecutive_failures), connectionStatus: String(row.connection_status) as SyncConnectionStatus,
    syncState: String(row.sync_state) as SyncState, lastErrorCode: optional(row, 'last_error_code'), lastErrorMessage: optional(row, 'last_error_message'),
  };
}

function jobFromRow(row: Row): SyncJob {
  return {
    id: String(row.id), accountId: String(row.account_id), mailbox: optional(row, 'mailbox'), mailboxRole: String(row.mailbox_role) as MailboxRole,
    reason: String(row.reason) as SyncJobReason, status: String(row.status) as SyncJob['status'], priority: Number(row.priority),
    notBefore: String(row.not_before), lockedBy: optional(row, 'locked_by'), lockedUntil: optional(row, 'locked_until'), attempts: Number(row.attempts),
    createdAt: String(row.created_at), startedAt: optional(row, 'started_at'), finishedAt: optional(row, 'finished_at'),
    syncedCount: row.synced_count === null ? undefined : Number(row.synced_count), newCount: row.new_count === null ? undefined : Number(row.new_count),
    updatedCount: row.updated_count === null ? undefined : Number(row.updated_count), deletedCount: row.deleted_count === null ? undefined : Number(row.deleted_count),
    errorCode: optional(row, 'error_code'), errorMessage: optional(row, 'error_message'),
  };
}

export class SyncStore {
  private readonly db: DatabaseSync;

  constructor(public readonly databasePath = configuredDatabasePath()) {
    if (databasePath !== ':memory:') mkdirSync(path.dirname(databasePath), { recursive: true });
    this.db = new DatabaseSync(databasePath, { timeout: 5_000 });
    this.db.exec('PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;');
    if (databasePath !== ':memory:') this.db.exec('PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;');
    ensureSchema(this.db);
  }

  close() { this.db.close(); }

  getDefaultPolicy(): SyncPolicySettings {
    const key = `sync_default_policy:${currentUserId() ?? '__legacy__'}`;
    const row = this.db.prepare('SELECT value FROM metadata WHERE key = ?').get(key) as Row | undefined;
    if (!row) return defaultSettings();
    try { return { ...defaultSettings(), ...JSON.parse(String(row.value)) as Partial<SyncPolicySettings> }; }
    catch { return defaultSettings(); }
  }

  updateDefaultPolicy(changes: Partial<SyncPolicySettings>): SyncPolicySettings {
    const updated = { ...this.getDefaultPolicy(), ...changes };
    const key = `sync_default_policy:${currentUserId() ?? '__legacy__'}`;
    this.db.prepare('INSERT OR REPLACE INTO metadata (key, value) VALUES (?, ?)').run(key, JSON.stringify(updated));
    return updated;
  }

  ensurePolicy(accountId: string): SyncPolicy {
    const now = new Date().toISOString();
    const defaults = { ...defaultSyncPolicy(accountId, now), ...this.getDefaultPolicy() };
    this.db.prepare(`INSERT OR IGNORE INTO sync_policies
      (account_id, enabled, interval_minutes, folder_mode, selected_mailboxes_json, sync_on_start, retry_on_recovery, notify_on_error, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`).run(accountId, Number(defaults.enabled), defaults.intervalMinutes, defaults.folderMode, JSON.stringify(defaults.selectedMailboxes), Number(defaults.syncOnStart), Number(defaults.retryOnRecovery), Number(defaults.notifyOnError), now);
    return this.getPolicy(accountId)!;
  }

  getPolicy(accountId: string): SyncPolicy | undefined {
    const row = this.db.prepare('SELECT * FROM sync_policies WHERE account_id = ?').get(accountId) as Row | undefined;
    return row ? policyFromRow(row) : undefined;
  }

  listPolicies(accountIds?: string[]): SyncPolicy[] {
    if (accountIds && accountIds.length === 0) return [];
    const rows = accountIds
      ? this.db.prepare(`SELECT * FROM sync_policies WHERE account_id IN (${accountIds.map(() => '?').join(',')}) ORDER BY account_id`).all(...accountIds) as Row[]
      : this.db.prepare('SELECT * FROM sync_policies ORDER BY account_id').all() as Row[];
    return rows.map(policyFromRow);
  }

  updatePolicy(accountId: string, changes: Partial<Omit<SyncPolicy, 'accountId' | 'updatedAt'>>): SyncPolicy {
    const current = this.ensurePolicy(accountId);
    const updated: SyncPolicy = { ...current, ...changes, accountId, updatedAt: new Date().toISOString() };
    this.db.prepare(`UPDATE sync_policies SET enabled = ?, interval_minutes = ?, folder_mode = ?, selected_mailboxes_json = ?,
      sync_on_start = ?, retry_on_recovery = ?, notify_on_error = ?, updated_at = ? WHERE account_id = ?`).run(
      Number(updated.enabled), updated.intervalMinutes, updated.folderMode, JSON.stringify(updated.selectedMailboxes),
      Number(updated.syncOnStart), Number(updated.retryOnRecovery), Number(updated.notifyOnError), updated.updatedAt, accountId,
    );
    if (!updated.enabled) {
      this.db.prepare("UPDATE sync_jobs SET status = 'cancelled', finished_at = ? WHERE account_id = ? AND status = 'queued'").run(updated.updatedAt, accountId);
      this.db.prepare("UPDATE mailbox_sync_states SET sync_state = 'paused', next_sync_at = NULL WHERE account_id = ?").run(accountId);
    } else {
      this.db.prepare("UPDATE mailbox_sync_states SET sync_state = CASE WHEN sync_state = 'paused' THEN 'idle' ELSE sync_state END WHERE account_id = ?").run(accountId);
    }
    return updated;
  }

  resumeAfterAuthorization(accountId: string) {
    const now = new Date().toISOString();
    this.db.prepare(`UPDATE mailbox_sync_states SET connection_status = 'connected', sync_state = 'idle', next_sync_at = ?,
      consecutive_failures = 0, last_error_code = NULL, last_error_message = NULL WHERE account_id = ? AND connection_status = 'authRequired'`).run(now, accountId);
  }

  listMailboxStates(accountId?: string): MailboxSyncState[] {
    const rows = accountId
      ? this.db.prepare('SELECT * FROM mailbox_sync_states WHERE account_id = ? ORDER BY mailbox').all(accountId) as Row[]
      : this.db.prepare('SELECT * FROM mailbox_sync_states ORDER BY account_id, mailbox').all() as Row[];
    return rows.map(stateFromRow);
  }

  getMailboxState(accountId: string, mailbox: string): MailboxSyncState | undefined {
    const row = this.db.prepare('SELECT * FROM mailbox_sync_states WHERE account_id = ? AND mailbox = ?').get(accountId, mailbox) as Row | undefined;
    return row ? stateFromRow(row) : undefined;
  }

  enqueueJob(input: { accountId: string; mailbox?: string; mailboxRole?: MailboxRole; reason: SyncJobReason; priority?: number; notBefore?: string }): SyncJob {
    const now = new Date().toISOString();
    const id = crypto.randomUUID();
    const mailboxRole = input.mailboxRole ?? 'inbox';
    const priority = input.priority ?? 0;
    const notBefore = input.notBefore ?? now;
    this.db.prepare(`INSERT OR IGNORE INTO sync_jobs
      (id, account_id, mailbox, mailbox_role, reason, status, priority, not_before, attempts, created_at)
      VALUES (?, ?, ?, ?, ?, 'queued', ?, ?, 0, ?)`).run(id, input.accountId, input.mailbox ?? null, mailboxRole, input.reason, priority, notBefore, now);
    this.db.prepare(`UPDATE sync_jobs SET priority = max(priority, ?), not_before = min(not_before, ?)
      WHERE account_id = ? AND coalesce(mailbox, '') = coalesce(?, '') AND mailbox_role = ? AND status = 'queued'`)
      .run(priority, notBefore, input.accountId, input.mailbox ?? null, mailboxRole);
    const row = this.db.prepare(`SELECT * FROM sync_jobs WHERE account_id = ? AND coalesce(mailbox, '') = coalesce(?, '') AND mailbox_role = ?
      AND status IN ('queued', 'running') ORDER BY created_at LIMIT 1`).get(input.accountId, input.mailbox ?? null, mailboxRole) as Row | undefined;
    if (!row) throw new Error('无法创建同步任务');
    return jobFromRow(row);
  }

  claimNextJob(workerId: string, leaseMs: number, now = new Date()): SyncJob | undefined {
    const nowIso = now.toISOString();
    const lockedUntil = new Date(now.getTime() + leaseMs).toISOString();
    this.db.exec('BEGIN IMMEDIATE');
    try {
      this.db.prepare(`UPDATE sync_jobs SET status = 'queued', locked_by = NULL, locked_until = NULL
        WHERE status = 'running' AND locked_until IS NOT NULL AND locked_until <= ?`).run(nowIso);
      const row = this.db.prepare(`SELECT * FROM sync_jobs WHERE status = 'queued' AND not_before <= ?
        ORDER BY priority DESC, created_at ASC LIMIT 1`).get(nowIso) as Row | undefined;
      if (!row) { this.db.exec('COMMIT'); return undefined; }
      this.db.prepare(`UPDATE sync_jobs SET status = 'running', locked_by = ?, locked_until = ?, attempts = attempts + 1,
        started_at = coalesce(started_at, ?) WHERE id = ?`).run(workerId, lockedUntil, nowIso, String(row.id));
      const claimed = this.db.prepare('SELECT * FROM sync_jobs WHERE id = ?').get(String(row.id)) as Row;
      this.db.exec('COMMIT');
      return jobFromRow(claimed);
    } catch (error) {
      this.db.exec('ROLLBACK');
      throw error;
    }
  }

  renewLease(jobId: string, workerId: string, leaseMs: number) {
    const lockedUntil = new Date(Date.now() + leaseMs).toISOString();
    const result = this.db.prepare("UPDATE sync_jobs SET locked_until = ? WHERE id = ? AND status = 'running' AND locked_by = ?").run(lockedUntil, jobId, workerId);
    return Number(result.changes) === 1;
  }

  markJobStarted(job: SyncJob, mailbox: string) {
    const now = new Date().toISOString();
    this.db.exec('BEGIN IMMEDIATE');
    try {
      const lease = this.db.prepare("SELECT 1 AS valid FROM sync_jobs WHERE id = ? AND status = 'running' AND locked_by = ?").get(job.id, job.lockedBy ?? null) as Row | undefined;
      if (!lease) throw new Error('同步任务租约已失效');
      this.db.prepare(`INSERT INTO mailbox_sync_states
        (account_id, mailbox, mailbox_role, last_seen_uid, last_attempt_at, consecutive_failures, connection_status, sync_state)
        VALUES (?, ?, ?, 0, ?, 0, 'connected', 'running')
        ON CONFLICT(account_id, mailbox) DO UPDATE SET mailbox_role = excluded.mailbox_role, last_attempt_at = excluded.last_attempt_at, sync_state = 'running', last_error_code = NULL, last_error_message = NULL`)
        .run(job.accountId, mailbox, job.mailboxRole, now);
      this.insertEvent('sync.started', job.accountId, job.id, { mailbox, mailboxRole: job.mailboxRole, reason: job.reason }, now);
      this.db.exec('COMMIT');
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }

  completeJob(job: SyncJob, result: { mailbox: string; uidValidity?: string; highestModseq?: string; lastSeenUid: number; synced: number; created: number; updated: number; deleted: number; messageChanges?: Array<{ before?: CachedMessage; after?: CachedMessage }> }, intervalMinutes: number) {
    const now = new Date(); const nowIso = now.toISOString(); const next = new Date(now.getTime() + intervalMinutes * 60_000).toISOString();
    this.db.exec('BEGIN IMMEDIATE');
    try {
      const completed = this.db.prepare(`UPDATE sync_jobs SET status = 'succeeded', finished_at = ?, locked_by = NULL, locked_until = NULL,
        synced_count = ?, new_count = ?, updated_count = ?, deleted_count = ?, error_code = NULL, error_message = NULL
        WHERE id = ? AND status = 'running' AND locked_by = ?`)
        .run(nowIso, result.synced, result.created, result.updated, result.deleted, job.id, job.lockedBy ?? null);
      if (Number(completed.changes) !== 1) throw new Error('同步任务租约已失效');
      this.db.prepare(`INSERT INTO mailbox_sync_states
        (account_id, mailbox, mailbox_role, uid_validity, last_seen_uid, highest_modseq, last_attempt_at, last_success_at, next_sync_at, consecutive_failures, connection_status, sync_state)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 'connected', 'idle')
        ON CONFLICT(account_id, mailbox) DO UPDATE SET mailbox_role = excluded.mailbox_role, uid_validity = excluded.uid_validity,
        last_seen_uid = excluded.last_seen_uid, highest_modseq = excluded.highest_modseq, last_attempt_at = excluded.last_attempt_at,
        last_success_at = excluded.last_success_at, next_sync_at = excluded.next_sync_at, consecutive_failures = 0,
        connection_status = 'connected', sync_state = 'idle', last_error_code = NULL, last_error_message = NULL`)
        .run(job.accountId, result.mailbox, job.mailboxRole, result.uidValidity ?? null, result.lastSeenUid, result.highestModseq ?? null, nowIso, nowIso, next);
      this.db.prepare("DELETE FROM mailbox_sync_states WHERE account_id = ? AND mailbox LIKE '@role:%' AND mailbox <> ?").run(job.accountId, result.mailbox);
      this.insertEvent('sync.completed', job.accountId, job.id, {
        mailbox: result.mailbox, mailboxRole: job.mailboxRole, synced: result.synced, created: result.created, updated: result.updated, deleted: result.deleted,
        messageChanges: (result.messageChanges ?? []).map((change) => ({
          ...(change.before ? { before: clientMessageSummary(change.before) } : {}),
          ...(change.after ? { after: clientMessageSummary(change.after) } : {}),
        })),
      }, nowIso);
      this.db.exec('COMMIT');
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }

  failJob(job: SyncJob, failure: { mailbox: string; code: string; message: string; authRequired: boolean }, retryMinutes?: number) {
    const now = new Date(); const nowIso = now.toISOString();
    const next = retryMinutes ? new Date(now.getTime() + retryMinutes * 60_000).toISOString() : undefined;
    const connectionStatus: SyncConnectionStatus = failure.authRequired ? 'authRequired' : 'unreachable';
    const syncState: SyncState = failure.authRequired ? 'paused' : 'backoff';
    this.db.exec('BEGIN IMMEDIATE');
    try {
      const failed = this.db.prepare(`UPDATE sync_jobs SET status = 'failed', finished_at = ?, locked_by = NULL, locked_until = NULL,
        error_code = ?, error_message = ? WHERE id = ? AND status = 'running' AND locked_by = ?`).run(nowIso, failure.code, failure.message, job.id, job.lockedBy ?? null);
      if (Number(failed.changes) !== 1) throw new Error('同步任务租约已失效');
      this.db.prepare(`INSERT INTO mailbox_sync_states
        (account_id, mailbox, mailbox_role, last_seen_uid, last_attempt_at, next_sync_at, consecutive_failures, connection_status, sync_state, last_error_code, last_error_message)
        VALUES (?, ?, ?, 0, ?, ?, 1, ?, ?, ?, ?)
        ON CONFLICT(account_id, mailbox) DO UPDATE SET last_attempt_at = excluded.last_attempt_at, next_sync_at = excluded.next_sync_at,
        consecutive_failures = mailbox_sync_states.consecutive_failures + 1, connection_status = excluded.connection_status,
        sync_state = excluded.sync_state, last_error_code = excluded.last_error_code, last_error_message = excluded.last_error_message`)
        .run(job.accountId, failure.mailbox, job.mailboxRole, nowIso, next ?? null, connectionStatus, syncState, failure.code, failure.message);
      this.insertEvent('sync.failed', job.accountId, job.id, { mailbox: failure.mailbox, mailboxRole: job.mailboxRole, code: failure.code, message: failure.message, nextSyncAt: next }, nowIso);
      this.db.exec('COMMIT');
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }

  cancelJob(job: SyncJob, mailbox: string) {
    const now = new Date().toISOString();
    this.db.exec('BEGIN IMMEDIATE');
    try {
      const cancelled = this.db.prepare(`UPDATE sync_jobs SET status = 'cancelled', finished_at = ?, locked_by = NULL, locked_until = NULL,
        error_code = NULL, error_message = NULL WHERE id = ? AND status = 'running' AND locked_by = ?`).run(now, job.id, job.lockedBy ?? null);
      if (Number(cancelled.changes) !== 1) throw new Error('同步任务租约已失效');
      this.db.prepare(`INSERT INTO mailbox_sync_states
        (account_id, mailbox, mailbox_role, last_seen_uid, consecutive_failures, connection_status, sync_state)
        VALUES (?, ?, ?, 0, 0, 'connected', 'paused')
        ON CONFLICT(account_id, mailbox) DO UPDATE SET sync_state = 'paused', next_sync_at = NULL`)
        .run(job.accountId, mailbox, job.mailboxRole);
      this.db.exec('COMMIT');
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }

  listJobs(input: { accountId?: string; limit?: number } = {}): SyncJob[] {
    const limit = Math.min(100, Math.max(1, input.limit ?? 20));
    const rows = input.accountId
      ? this.db.prepare('SELECT * FROM sync_jobs WHERE account_id = ? ORDER BY created_at DESC LIMIT ?').all(input.accountId, limit) as Row[]
      : this.db.prepare('SELECT * FROM sync_jobs ORDER BY created_at DESC LIMIT ?').all(limit) as Row[];
    return rows.map(jobFromRow);
  }

  getJob(id: string) {
    const row = this.db.prepare('SELECT * FROM sync_jobs WHERE id = ?').get(id) as Row | undefined;
    return row ? jobFromRow(row) : undefined;
  }

  listEvents(afterId: number, limit = 100): SyncEvent[] {
    const rows = this.db.prepare('SELECT * FROM sync_events WHERE id > ? ORDER BY id LIMIT ?').all(afterId, Math.min(500, Math.max(1, limit))) as Row[];
    return rows.map((row) => ({ id: Number(row.id), type: String(row.event_type) as SyncEvent['type'], accountId: String(row.account_id), jobId: optional(row, 'job_id'), payload: JSON.parse(String(row.payload_json)), createdAt: String(row.created_at) }));
  }

  latestEventId() {
    const row = this.db.prepare('SELECT coalesce(max(id), 0) AS id FROM sync_events').get() as Row;
    return Number(row.id);
  }

  recordMessageCreated(account: MailAccount, messages: CachedMessage[]) {
    if (messages.length === 0) return;
    const now = new Date().toISOString();
    this.db.exec('BEGIN IMMEDIATE');
    try {
      for (const message of messages) this.insertEvent('message.created', account.id, undefined, { message: integrationMessageSummary(message, account.email) }, now);
      this.db.exec('COMMIT');
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }

  pruneEvents(before: string) { this.db.prepare('DELETE FROM sync_events WHERE created_at < ?').run(before); }

  heartbeat(workerId: string, processId: number, hostName: string, startedAt: string) {
    const now = new Date().toISOString();
    this.db.prepare(`INSERT INTO sync_worker_heartbeats (worker_id, process_id, host_name, started_at, heartbeat_at) VALUES (?, ?, ?, ?, ?)
      ON CONFLICT(worker_id) DO UPDATE SET heartbeat_at = excluded.heartbeat_at`).run(workerId, processId, hostName, startedAt, now);
  }

  removeHeartbeat(workerId: string) { this.db.prepare('DELETE FROM sync_worker_heartbeats WHERE worker_id = ?').run(workerId); }

  workerHealth() {
    const workers = (this.db.prepare('SELECT * FROM sync_worker_heartbeats ORDER BY heartbeat_at DESC').all() as Row[]).map((row) => ({
      workerId: String(row.worker_id), processId: Number(row.process_id), hostName: String(row.host_name), startedAt: String(row.started_at), heartbeatAt: String(row.heartbeat_at),
    }));
    const queue = this.db.prepare(`SELECT count(*) AS queued, min(created_at) AS oldest_queued_at FROM sync_jobs WHERE status = 'queued'`).get() as Row;
    return { workers, queuedJobs: Number(queue.queued), oldestQueuedAt: optional(queue, 'oldest_queued_at') };
  }

  deleteAccountData(accountId: string) {
    this.db.exec('BEGIN IMMEDIATE');
    try {
      this.db.prepare('DELETE FROM sync_jobs WHERE account_id = ?').run(accountId);
      this.db.prepare('DELETE FROM mailbox_sync_states WHERE account_id = ?').run(accountId);
      this.db.prepare('DELETE FROM sync_policies WHERE account_id = ?').run(accountId);
      this.db.prepare('DELETE FROM sync_events WHERE account_id = ?').run(accountId);
      this.db.exec('COMMIT');
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }

  private insertEvent(type: SyncEvent['type'], accountId: string, jobId: string | undefined, payload: Record<string, unknown>, createdAt: string) {
    this.db.prepare('INSERT INTO sync_events (event_type, account_id, job_id, payload_json, created_at) VALUES (?, ?, ?, ?, ?)')
      .run(type, accountId, jobId ?? null, JSON.stringify(payload), createdAt);
  }
}

let defaultStore: SyncStore | undefined;
let unregisterCloser: (() => void) | undefined;
export function getSyncStore() {
  if (!defaultStore) {
    defaultStore = new SyncStore();
    unregisterCloser = registerAuxiliaryStoreCloser(() => closeSyncStore());
  }
  return defaultStore;
}
export function closeSyncStore() {
  const store = defaultStore; defaultStore = undefined;
  unregisterCloser?.(); unregisterCloser = undefined;
  store?.close();
}
