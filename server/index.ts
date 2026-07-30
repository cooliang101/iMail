import path from 'node:path';
import { fork, type ChildProcess } from 'node:child_process';
import { createServer } from 'node:http';
import { fileURLToPath, pathToFileURL } from 'node:url';
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
      syncWorker = fork(fileURLToPath(new URL('./sync/worker.ts', import.meta.url)), [], {
        execArgv: ['--import', 'tsx'], stdio: ['inherit', 'inherit', 'inherit', 'ipc'], env: { ...process.env, IMAIL_SYNC_WORKER_MODE: 'child' },
      });
      syncWorker.once('exit', (code, signal) => {
        if (closing || !server.listening) return;
        console.error(`[sync-worker] exited unexpectedly (${signal ?? code ?? 'unknown'}); restarting`);
        workerRestartTimer = setTimeout(spawnWorker, 1_000);
      });
    };
    spawnWorker();
  }
  server.once('close', () => { closing = true; if (workerRestartTimer) clearTimeout(workerRestartTimer); if (syncWorker?.connected) syncWorker.disconnect(); closeAuthStore(); });
  return server.listen(port, host, () => console.log(`iMail API running at http://${host}:${port}`));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) startServer();
