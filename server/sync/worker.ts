import path from 'node:path';
import { closeStore } from '../store.js';
import { closeSyncStore } from './store.js';
import { startSyncWorker } from './worker-runtime.js';
import { installRuntimeErrorLogging, runtimeLog } from '../runtime-logging.js';

export function parentProcessAlive(parentPid: number, signal: typeof process.kill = process.kill) {
  if (!Number.isSafeInteger(parentPid) || parentPid <= 0) return false;
  try { signal(parentPid, 0); return true; }
  catch (error) { return (error as NodeJS.ErrnoException).code === 'EPERM'; }
}

export function runSyncWorkerProcess() {
  installRuntimeErrorLogging('sync-worker');
  const worker = startSyncWorker();
  let closing = false;
  let parentTimer: NodeJS.Timeout | undefined;
  const shutdown = async () => {
    if (closing) return;
    closing = true;
    if (parentTimer) clearInterval(parentTimer);
    await worker.close();
    closeSyncStore(); closeStore();
  };
  process.once('SIGINT', () => { void shutdown().finally(() => process.exit(0)); });
  process.once('SIGTERM', () => { void shutdown().finally(() => process.exit(0)); });
  process.once('disconnect', () => { void shutdown().finally(() => process.exit(0)); });
  const parentPid = Number(process.env.IMAIL_PARENT_PID);
  if (Number.isSafeInteger(parentPid) && parentPid > 0) {
    parentTimer = setInterval(() => {
      if (!parentProcessAlive(parentPid)) void shutdown().finally(() => process.exit(0));
    }, 1_000);
    parentTimer.unref();
  }
  runtimeLog('INFO', 'sync-worker.started', `worker=${worker.workerId}`);
  return worker;
}

const entry = process.argv[1] ? path.resolve(process.argv[1]) : '';
if (path.basename(entry) === 'worker.ts' && path.basename(path.dirname(entry)) === 'sync') runSyncWorkerProcess();
