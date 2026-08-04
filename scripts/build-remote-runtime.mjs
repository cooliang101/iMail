import { mkdirSync, rmSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.join(root, 'server-runtime');
rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });

for (const [entry, outfile, format] of [
  ['server/remote-entry.ts', 'imail-server.cjs', 'cjs'],
  ['server/sync/worker.ts', 'imail-worker.cjs', 'cjs'],
  ['scripts/backup-data.mjs', 'imail-backup.mjs', 'esm'],
  ['scripts/prepare-restore.mjs', 'imail-restore.mjs', 'esm'],
  ['server/maintenance/upgrade-preflight.ts', 'imail-upgrade-preflight.mjs', 'esm'],
]) {
  await build({
    entryPoints: [path.join(root, entry)],
    outfile: path.join(output, outfile),
    bundle: true,
    platform: 'node',
    format,
    target: 'node24',
    sourcemap: false,
    minify: false,
    logLevel: 'info',
    define: { 'process.env.npm_package_version': JSON.stringify(process.env.npm_package_version ?? '0.0.1') },
  });
}
