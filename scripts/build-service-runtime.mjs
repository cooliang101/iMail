import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const runtimeDir = path.join(root, 'desktop-runtime');
const binaryDir = path.join(root, 'src-tauri', 'binaries');
const extension = process.platform === 'win32' ? '.exe' : '';

function targetTriple() {
  const output = execFileSync('rustc', ['-Vv'], { encoding: 'utf8' });
  const match = output.match(/^host:\s*(\S+)$/m);
  if (!match) throw new Error('无法从 rustc -Vv 确定 Tauri 目标平台');
  return match[1];
}

rmSync(runtimeDir, { recursive: true, force: true });
mkdirSync(runtimeDir, { recursive: true });
mkdirSync(binaryDir, { recursive: true });

const bundle = path.join(runtimeDir, 'imail-service.cjs');
await build({
  entryPoints: [path.join(root, 'server', 'sea-entry.ts')],
  outfile: bundle,
  bundle: true,
  platform: 'node',
  format: 'cjs',
  target: `node${process.versions.node.split('.')[0]}`,
  sourcemap: false,
  minify: false,
  logLevel: 'info',
  define: { 'process.env.npm_package_version': JSON.stringify(process.env.npm_package_version ?? '0.1.0') },
});

const seaBlob = path.join(runtimeDir, 'imail-service.blob');
const seaConfig = path.join(runtimeDir, 'sea-config.json');
writeFileSync(seaConfig, JSON.stringify({
  main: bundle,
  output: seaBlob,
  disableExperimentalSEAWarning: true,
  useSnapshot: false,
  useCodeCache: false,
}, null, 2));

execFileSync(process.execPath, ['--experimental-sea-config', seaConfig], { stdio: 'inherit' });

const rustTargetTriple = targetTriple();
const output = path.join(binaryDir, `imail-service-${rustTargetTriple}${extension}`);
if (existsSync(output)) rmSync(output);
copyFileSync(process.execPath, output);

const postject = path.join(root, 'node_modules', 'postject', 'dist', 'cli.js');
const args = [
  postject,
  output,
  'NODE_SEA_BLOB',
  seaBlob,
  '--sentinel-fuse',
  'NODE_SEA_FUSE_fce680ab2cc467b6e072b8b5df1996b2',
];
if (process.platform === 'darwin') args.push('--macho-segment-name', 'NODE_SEA');
execFileSync(process.execPath, args, { stdio: 'inherit' });

if (process.platform !== 'win32') execFileSync('chmod', ['755', output]);
if (process.platform === 'win32') {
  const architecture = process.arch === 'arm64' ? 'aarch64' : 'x86_64';
  const windowsBundleTarget = `${architecture}-pc-windows-msvc`;
  const tauriBundleOutput = path.join(binaryDir, `imail-service-${windowsBundleTarget}.exe`);
  if (tauriBundleOutput !== output) {
    if (existsSync(tauriBundleOutput)) rmSync(tauriBundleOutput);
    copyFileSync(output, tauriBundleOutput);
    console.log(`iMail Tauri bundle alias: ${tauriBundleOutput}`);
  }
  execFileSync('cargo', [
    'build', '--release', '--locked', '--manifest-path', path.join(root, 'src-tauri', 'cleanup-helper', 'Cargo.toml'),
    '--target', windowsBundleTarget,
  ], { cwd: root, stdio: 'inherit' });
  execFileSync('cargo', [
    'build', '--locked', '--manifest-path', path.join(root, 'src-tauri', 'cleanup-helper', 'Cargo.toml'),
    '--target', windowsBundleTarget,
  ], { cwd: root, stdio: 'inherit' });
  const managerSource = path.join(root, 'src-tauri', 'cleanup-helper', 'target', windowsBundleTarget, 'release', 'imail-service-manager.exe');
  const managerOutput = path.join(binaryDir, `imail-service-manager-${windowsBundleTarget}.exe`);
  if (existsSync(managerOutput)) rmSync(managerOutput);
  copyFileSync(managerSource, managerOutput);
  console.log(`iMail service manager: ${managerOutput}`);
}
console.log(`iMail service runtime: ${output}`);
