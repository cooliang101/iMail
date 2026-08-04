import path from 'node:path';
import { fork, spawn, type ChildProcess } from 'node:child_process';
import { createServer, type Server } from 'node:http';
import { app } from './app.js';
import { attachGatewayWebSocket } from './gateway/websocket.js';
import { closeAuthStore } from './auth/http.js';

export { app, createApp } from './app.js';

const port = Number(process.env.PORT ?? 8787);
const host = process.env.HOST ?? '127.0.0.1';

export function startServer() {
  const server = createServer(app);
  attachGatewayWebSocket(server);
  let syncWorker: ChildProcess | undefined;
  let workerRestartTimer: NodeJS.Timeout | undefined;
  let closing = false;
  if (process.env.IMAIL_SYNC_WORKER_MODE !== 'external' && process.env.IMAIL_SYNC_WORKER_MODE !== 'disabled') {
    const spawnWorker = () => {
      const configuredWorker = process.env.IMAIL_WORKER_ENTRY;
      const workerEntry = configuredWorker ? path.resolve(configuredWorker) : path.resolve('server/sync/worker.ts');
      const environment = { ...process.env, IMAIL_SYNC_WORKER_MODE: 'child', IMAIL_PARENT_PID: String(process.pid) };
      syncWorker = process.env.IMAIL_PACKAGED_SERVICE === 'true'
        ? spawn(process.execPath, ['--sync-worker'], { stdio: ['ignore', 'inherit', 'inherit'], env: environment })
        : fork(workerEntry, [], {
          execArgv: configuredWorker ? [] : ['--import', 'tsx'], stdio: ['inherit', 'inherit', 'inherit', 'ipc'], env: environment,
        });
      syncWorker.once('exit', (code, signal) => {
        if (closing || !server.listening) return;
        console.error(`[sync-worker] exited unexpectedly (${signal ?? code ?? 'unknown'}); restarting`);
        workerRestartTimer = setTimeout(spawnWorker, 1_000);
      });
    };
    spawnWorker();
  }
  server.once('close', () => {
    closing = true;
    if (workerRestartTimer) clearTimeout(workerRestartTimer);
    if (syncWorker?.connected) syncWorker.disconnect();
    else if (syncWorker && !syncWorker.killed) syncWorker.kill('SIGTERM');
    closeAuthStore();
  });
  return server.listen(port, host, () => console.log(`iMail API running at http://${host}:${port}`));
}

export function installServerSignalHandlers(server: Server) {
  let stopping = false;
  const shutdown = () => {
    if (stopping) return;
    stopping = true;
    const forced = setTimeout(() => process.exit(1), 10_000);
    forced.unref();
    server.close((error) => {
      clearTimeout(forced);
      process.exit(error ? 1 : 0);
    });
  };
  process.once('SIGINT', shutdown);
  process.once('SIGTERM', shutdown);
  process.once('disconnect', shutdown);
}
