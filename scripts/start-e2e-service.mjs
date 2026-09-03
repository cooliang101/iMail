import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';

const dataDirectory = await mkdtemp(path.join(tmpdir(), 'imail-frontend-e2e-'));
const cargoTargetArguments = process.platform === 'win32'
  ? ['--target', 'x86_64-pc-windows-msvc']
  : [];
const cargo = spawn('cargo', [
  'run', ...cargoTargetArguments, '--locked', '-p', 'imail-http-service', '--',
  '--data-dir', dataDirectory,
  '--host', '127.0.0.1',
  '--port', '18787',
], { cwd: path.resolve(import.meta.dirname, '..'), stdio: 'inherit', windowsHide: true });

let stopping = false;
async function stop(signal) {
  if (stopping) return;
  stopping = true;
  cargo.kill(signal === 'SIGINT' ? 'SIGINT' : 'SIGTERM');
  await rm(dataDirectory, { recursive: true, force: true });
}

process.on('SIGINT', () => void stop('SIGINT'));
process.on('SIGTERM', () => void stop('SIGTERM'));
cargo.on('exit', async (code) => {
  await rm(dataDirectory, { recursive: true, force: true });
  process.exit(code ?? 0);
});
