import { pathToFileURL } from 'node:url';
import path from 'node:path';
import { closeStore } from '../store.js';
import { closeSyncStore } from './store.js';
import { startSyncWorker } from './worker-runtime.js';

export function runSyncWorkerProcess() {
  const worker = startSyncWorker();
  const shutdown = async () => {
    await worker.close();
    closeSyncStore(); closeStore();
  };
  process.once('SIGINT', () => { void shutdown().finally(() => process.exit(0)); });
  process.once('SIGTERM', () => { void shutdown().finally(() => process.exit(0)); });
  process.once('disconnect', () => { void shutdown().finally(() => process.exit(0)); });
  console.log(`[sync-worker] running as ${worker.workerId}`);
  return worker;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) runSyncWorkerProcess();
