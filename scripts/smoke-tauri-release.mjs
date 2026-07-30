import { spawn } from 'node:child_process';
import path from 'node:path';
import { createServer } from 'node:net';

const port = await new Promise((resolve, reject) => {
  const probe = createServer();
  probe.once('error', reject);
  probe.listen(0, '127.0.0.1', () => {
    const address = probe.address();
    if (!address || typeof address === 'string') { probe.close(); reject(new Error('failed to allocate release smoke-test port')); return; }
    probe.close(() => resolve(address.port));
  });
});

if (process.platform !== 'win32') throw new Error('This release smoke test currently targets the Windows artifact');
const executable = path.resolve('src-tauri', 'target', 'release', 'imail.exe');
const child = spawn(executable, [], {
  stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, IMAIL_DESKTOP_SMOKE_TEST: 'true', IMAIL_DESKTOP_PORT: String(port) },
});
let output = '';
child.stdout.on('data', (chunk) => { output += chunk; });
child.stderr.on('data', (chunk) => { output += chunk; });
const result = await Promise.race([
  new Promise((resolve) => child.once('exit', (code, signal) => resolve({ code, signal }))),
  new Promise((_, reject) => setTimeout(() => reject(new Error(`packaged app did not exit after smoke test\n${output}`)), 20_000)),
]);
if (result.code !== 0) throw new Error(`packaged app smoke test failed (${JSON.stringify(result)})\n${output}`);
await new Promise((resolve) => setTimeout(resolve, 500));
let portReleased = false;
try { await fetch(`http://127.0.0.1:${port}/`, { signal: AbortSignal.timeout(500) }); }
catch { portReleased = true; }
if (!portReleased) throw new Error(`packaged app left its sidecar listening on port ${port}`);
console.log(JSON.stringify({ exitCode: result.code, port, portReleased }));
