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

// Tauri invokes this script from npm without Node's --env-file flag. Load the
// local build configuration explicitly, then compile only the desktop OAuth
// client registration into the SEA bundle. The .env file itself is never
// copied into the application package.
const localEnvironment = path.join(root, '.env');
if (existsSync(localEnvironment)) process.loadEnvFile(localEnvironment);

function configuredValue(...names) {
  for (const name of names) {
    const value = process.env[name]?.trim();
    if (value) return value;
  }
  return undefined;
}

const desktopOAuth = {
  GOOGLE_OAUTH_CLIENT_ID: configuredValue('GOOGLE_OAUTH_DESKTOP_CLIENT_ID', 'GOOGLE_OAUTH_CLIENT_ID'),
  // Google includes this field in Desktop app credentials and may require it
  // at the token endpoint. It is an identifier-like public-client parameter,
  // not a secret that a distributed executable can protect.
  GOOGLE_OAUTH_CLIENT_SECRET: configuredValue('GOOGLE_OAUTH_DESKTOP_CLIENT_SECRET'),
  MICROSOFT_OAUTH_CLIENT_ID: configuredValue('MICROSOFT_OAUTH_DESKTOP_CLIENT_ID', 'MICROSOFT_OAUTH_CLIENT_ID'),
};

const desktopOAuthDefines = Object.fromEntries(Object.entries(desktopOAuth)
  .filter(([, value]) => value !== undefined)
  .map(([name, value]) => [`process.env.${name}`, JSON.stringify(value)]));

const configuredProviders = [
  desktopOAuth.GOOGLE_OAUTH_CLIENT_ID && desktopOAuth.GOOGLE_OAUTH_CLIENT_SECRET ? 'Google public client + PKCE' : undefined,
  desktopOAuth.MICROSOFT_OAUTH_CLIENT_ID ? 'Microsoft public client + PKCE' : undefined,
].filter(Boolean);
console.log(`iMail desktop OAuth clients: ${configuredProviders.join(', ') || 'none'}`);

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
  define: {
    'process.env.npm_package_version': JSON.stringify(process.env.npm_package_version ?? '0.0.1'),
    // Microsoft uses a secretless public client. Google Desktop credentials
    // may require their non-confidential client_secret field in addition to PKCE.
    'process.env.MICROSOFT_OAUTH_CLIENT_SECRET': 'undefined',
    ...desktopOAuthDefines,
  },
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
