import { mkdir, rm } from 'node:fs/promises';
import { build } from 'esbuild';

const outputDirectory = 'desktop-runtime';
await rm(outputDirectory, { recursive: true, force: true });
await mkdir(outputDirectory, { recursive: true });

const shared = {
  bundle: true,
  platform: 'node',
  format: 'cjs',
  target: 'node22',
  sourcemap: true,
  packages: 'bundle',
  logLevel: 'info',
};

await Promise.all([
  build({ ...shared, entryPoints: ['server/desktop/entry.ts'], outfile: `${outputDirectory}/server.cjs` }),
  build({ ...shared, entryPoints: ['server/desktop/worker-entry.ts'], outfile: `${outputDirectory}/worker.cjs` }),
]);
