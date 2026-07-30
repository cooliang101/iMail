import { copyFile, mkdir } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';

const hostTriple = execFileSync('rustc', ['--print', 'host-tuple'], { encoding: 'utf8' }).trim();
if (!hostTriple) throw new Error('无法确定 Rust target triple');
const extension = process.platform === 'win32' ? '.exe' : '';
const directory = path.resolve('src-tauri', 'binaries');
await mkdir(directory, { recursive: true });
const triples = new Set([hostTriple]);
if (process.platform === 'win32') {
  const architecture = process.arch === 'arm64' ? 'aarch64' : process.arch === 'x64' ? 'x86_64' : process.arch;
  triples.add(`${architecture}-pc-windows-msvc`);
}
for (const triple of triples) {
  const target = path.join(directory, `imail-node-${triple}${extension}`);
  await copyFile(process.execPath, target);
  console.log(`Prepared Node ${process.version} sidecar: ${target}`);
}
