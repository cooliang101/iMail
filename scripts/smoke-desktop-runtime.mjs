import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { createServer } from 'node:net';

const port = await new Promise((resolve, reject) => {
  const probe = createServer();
  probe.once('error', reject);
  probe.listen(0, '127.0.0.1', () => {
    const address = probe.address();
    if (!address || typeof address === 'string') { probe.close(); reject(new Error('failed to allocate smoke-test port')); return; }
    probe.close(() => resolve(address.port));
  });
});

const triple = String(process.env.IMAIL_TARGET_TRIPLE || '').trim()
  || String((await import('node:child_process')).execFileSync('rustc', ['--print', 'host-tuple'], { encoding: 'utf8' })).trim();
const extension = process.platform === 'win32' ? '.exe' : '';
const sidecar = path.resolve('src-tauri', 'binaries', `imail-node-${triple}${extension}`);
const serverEntry = path.resolve('desktop-runtime', 'server.cjs');
const workerEntry = path.resolve('desktop-runtime', 'worker.cjs');
const dataDirectory = await mkdtemp(path.join(tmpdir(), 'imail-desktop-smoke-'));
const startupToken = randomUUID();
const child = spawn(sidecar, [serverEntry], {
  stdio: ['ignore', 'pipe', 'pipe'],
  env: {
    ...process.env,
    NODE_ENV: 'production', PORT: String(port), HOST: '127.0.0.1',
    IMAIL_DESKTOP_MODE: 'true', IMAIL_DESKTOP_STARTUP_TOKEN: startupToken,
    IMAIL_DATA_DIR: dataDirectory, IMAIL_WEB_DIR: path.resolve('dist'), IMAIL_WORKER_ENTRY: workerEntry,
    IMAIL_SYNC_WORKER_MODE: 'child', CORS_ORIGIN: `http://127.0.0.1:${port}`, FRONTEND_URL: `http://127.0.0.1:${port}`,
  },
});
let output = '';
child.stdout.on('data', (chunk) => { output += chunk; });
child.stderr.on('data', (chunk) => { output += chunk; });

try {
  let health;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/api/desktop-health`, { headers: { 'x-imail-startup-token': startupToken } });
      if (response.ok) { health = await response.json(); break; }
    } catch { /* sidecar is still starting */ }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  if (!health?.ok) throw new Error(`sidecar did not become healthy\n${output}`);
  const root = await fetch(`http://127.0.0.1:${port}/`);
  const html = await root.text();
  const rejected = await fetch(`http://127.0.0.1:${port}/api/desktop-health`, { headers: { 'x-imail-startup-token': 'wrong' } });
  if (!root.ok || !html.includes('<div id="root"></div>')) throw new Error(`desktop static application was not served (${root.status})\n${html.slice(0, 500)}\n${output}`);
  if (rejected.status !== 401) throw new Error(`invalid startup token returned ${rejected.status}`);
  console.log(JSON.stringify({ health: health.ok, service: health.service, rootStatus: root.status, invalidTokenStatus: rejected.status }));
} finally {
  child.kill('SIGTERM');
  await Promise.race([
    new Promise((resolve) => child.once('exit', resolve)),
    new Promise((resolve) => setTimeout(resolve, 2_000)),
  ]);
  if (child.exitCode === null) child.kill('SIGKILL');
  await rm(dataDirectory, { recursive: true, force: true });
}
