import { spawn } from 'node:child_process';
import path from 'node:path';

if (process.platform !== 'win32') throw new Error('This release smoke test currently targets the Windows artifact');
const root = path.resolve(import.meta.dirname, '..');
const cargoTargetDir = process.env.CARGO_TARGET_DIR || path.join(root, 'src-tauri', 'target');
const executable = path.join(cargoTargetDir, 'x86_64-pc-windows-msvc', 'release', 'imail.exe');
const child = spawn(executable, [], {
  stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, IMAIL_DESKTOP_SMOKE_TEST: 'true' },
});
let output = '';
child.stdout.on('data', (chunk) => { output += chunk; });
child.stderr.on('data', (chunk) => { output += chunk; });
const result = await Promise.race([
  new Promise((resolve) => child.once('exit', (code, signal) => resolve({ code, signal }))),
  new Promise((_, reject) => setTimeout(() => reject(new Error(`packaged app did not exit after smoke test\n${output}`)), 20_000)),
]);
if (result.code !== 0) throw new Error(`packaged app smoke test failed (${JSON.stringify(result)})\n${output}`);
console.log(JSON.stringify({ exitCode: result.code, embeddedRustService: true, bundledNodeRuntime: false }));
