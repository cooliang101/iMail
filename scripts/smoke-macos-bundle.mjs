import { execFile } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { access, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const root = path.resolve(import.meta.dirname, '..');
if (process.platform !== 'darwin') throw new Error('macOS bundle 冒烟只能在 macOS 发布机运行');

const bundleRoot = path.join(root, 'src-tauri', 'target', 'release', 'bundle', 'macos');
const appName = (await readdir(bundleRoot)).find((name) => name.endsWith('.app'));
if (!appName) throw new Error('缺少 macOS .app，请先执行 npm run build:desktop:macos');
const appBundle = path.join(bundleRoot, appName);
const executableDirectory = path.join(appBundle, 'Contents', 'MacOS');
const names = await readdir(executableDirectory);
const serviceName = names.find((name) => name === 'imail-service');
const desktopName = names.find((name) => name.toLowerCase() === 'imail')
  ?? names.find((name) => name !== serviceName && !name.startsWith('.'));
if (!serviceName || !desktopName) throw new Error(`应用包缺少桌面程序或服务 sidecar：${names.join(', ')}`);
const service = path.join(executableDirectory, serviceName);
const desktop = path.join(executableDirectory, desktopName);

await execFileAsync('codesign', ['--verify', '--deep', '--strict', '--verbose=2', appBundle]);
const signature = await execFileAsync('codesign', ['--display', '--verbose=4', service]);
const signatureDetails = `${signature.stdout}\n${signature.stderr}`;
if (!signatureDetails.includes('runtime')) throw new Error('服务 sidecar 未启用 hardened runtime');
const entitlements = await execFileAsync('codesign', ['--display', '--entitlements', ':-', service]);
const entitlementDetails = `${entitlements.stdout}\n${entitlements.stderr}`;
if (!entitlementDetails.includes('allow-jit')) {
  throw new Error('服务 sidecar 缺少 Node/V8 所需的 JIT entitlement');
}
if (entitlementDetails.includes('allow-unsigned-executable-memory')) {
  throw new Error('服务 sidecar 不应申请未签名可执行内存权限');
}

await execFileAsync(desktop, [], {
  cwd: executableDirectory,
  env: { ...process.env, IMAIL_DESKTOP_SMOKE_TEST: 'true' },
  timeout: 30_000,
});

const temporary = await mkdtemp(path.join(tmpdir(), 'imail-macos-bundle-'));
const dataDir = path.join(temporary, 'data');
const controlFile = path.join(temporary, 'control-token');
const token = `${randomUUID().replaceAll('-', '')}${randomUUID().replaceAll('-', '')}`;
const instanceId = randomUUID();
let child;

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function freePort() {
  const { createServer } = await import('node:net');
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      const port = typeof address === 'object' && address ? address.port : 0;
      server.close((error) => error ? reject(error) : resolve(port));
    });
  });
}

async function waitFor(check, description, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const result = await check();
      if (result) return result;
    } catch (error) { lastError = error; }
    await delay(250);
  }
  throw new Error(`${description}超时${lastError ? `：${lastError}` : ''}`);
}

try {
  await access(service);
  await mkdir(dataDir, { recursive: true });
  await writeFile(controlFile, token);
  await writeFile(path.join(dataDir, 'instance-id'), `${instanceId}\n`);
  const port = await freePort();
  const { spawn } = await import('node:child_process');
  child = spawn(service, [
    '--data-dir', dataDir,
    '--host', '127.0.0.1',
    '--port', String(port),
    '--daemon-control-file', controlFile,
  ], {
    cwd: executableDirectory,
    env: process.env,
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  let stderr = '';
  child.stderr.on('data', (chunk) => { stderr += chunk.toString(); });
  const info = await waitFor(async () => {
    if (child.exitCode !== null) throw new Error(`sidecar 已退出（${child.exitCode}）：${stderr.trim()}`);
    const response = await fetch(`http://127.0.0.1:${port}/api/system/info`);
    return response.ok ? response.json() : undefined;
  }, '已签名 sidecar 启动');
  if (info.service !== 'imail' || info.instanceId !== instanceId || !info.capabilities?.syncWorker) {
    throw new Error(`sidecar 身份或能力异常：${JSON.stringify(info)}`);
  }
  const workerPid = await waitFor(async () => {
    const { stdout: processes } = await execFileAsync('ps', ['-axo', 'pid=,ppid=,command=']);
    for (const line of processes.split('\n')) {
      const match = line.trim().match(/^(\d+)\s+(\d+)\s+(.+)$/);
      if (match && Number(match[2]) === child.pid && match[3].includes('--sync-worker')) return Number(match[1]);
    }
    return undefined;
  }, '独立同步 Worker 启动');
  const shutdown = await fetch(`http://127.0.0.1:${port}/api/system/shutdown`, {
    method: 'POST',
    headers: { 'X-iMail-Daemon-Token': token },
  });
  if (!shutdown.ok) throw new Error(`sidecar 停止请求失败：HTTP ${shutdown.status}`);
  await waitFor(() => child.exitCode !== null, 'sidecar 优雅退出');
  await waitFor(async () => {
    const { stdout: processes } = await execFileAsync('ps', ['-axo', 'pid=']);
    return !processes.split('\n').some((value) => Number(value.trim()) === workerPid);
  }, '同步 Worker 随 API 退出');
  console.log(JSON.stringify({
    ok: true,
    deepSignature: true,
    hardenedRuntime: true,
    desktopLaunch: true,
    signedSidecarLaunch: true,
    allowJit: true,
    syncWorker: true,
    gracefulShutdown: true,
  }));
} catch (reason) {
  const error = reason instanceof Error ? reason : new Error(String(reason));
  const serviceLog = await readFile(path.join(dataDir, 'service.log'), 'utf8').catch(() => '');
  if (serviceLog) error.message += `\nserviceLog=${serviceLog}`;
  throw error;
} finally {
  if (child && child.exitCode === null) child.kill('SIGTERM');
  await delay(250);
  await rm(temporary, { recursive: true, force: true });
}

await import('./smoke-macos-launch-agent.mjs');
