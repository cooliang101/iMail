import { execFile } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { access, chmod, copyFile, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const root = path.resolve(import.meta.dirname, '..');
if (process.platform !== 'darwin') throw new Error('LaunchAgent 冒烟只能在 macOS 发布机运行');

const bundleRoot = path.join(root, 'src-tauri', 'target', 'release', 'bundle', 'macos');
const appName = (await readdir(bundleRoot)).find((name) => name.endsWith('.app'));
if (!appName) throw new Error('缺少 macOS .app，请先执行 npm run build:desktop:macos');
const executableDirectory = path.join(bundleRoot, appName, 'Contents', 'MacOS');
const bundledNames = await readdir(executableDirectory);
const bundledService = path.join(executableDirectory, 'imail-service');
const desktopName = bundledNames.find((name) => name.toLowerCase() === 'imail')
  ?? bundledNames.find((name) => name !== 'imail-service' && !name.startsWith('.'));
if (!desktopName) throw new Error('应用包缺少桌面 supervisor');
const desktop = path.join(executableDirectory, desktopName);

const serviceRoot = path.join(homedir(), 'Library', 'Application Support', 'com.cooliang.imail', 'local-service');
const launchAgents = path.join(homedir(), 'Library', 'LaunchAgents');
const plist = path.join(launchAgents, 'com.cooliang.imail.service.plist');
const configPath = path.join(serviceRoot, 'daemon.json');
const runtime = path.join(serviceRoot, 'runtime');
const dataDir = path.join(serviceRoot, 'data');
const logs = path.join(serviceRoot, 'logs');
const controlFile = path.join(serviceRoot, 'control-token');
const enabledFile = path.join(serviceRoot, 'enabled');
const supervisorLockFile = path.join(serviceRoot, 'supervisor.lock');
const manifest = JSON.parse(await readFile(path.join(root, 'frontend', 'package.json'), 'utf8'));
const serviceExecutable = path.join(runtime, `imail-service-${manifest.version}`);
const instanceId = randomUUID();
const supervisorId = randomUUID();
const token = `${randomUUID().replaceAll('-', '')}${randomUUID().replaceAll('-', '')}`;
const port = 8787;
let bootstrapped = false;
let createdRoot = false;
let createdPlist = false;

const exists = (target) => access(target).then(() => true, () => false);
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

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

async function processRows() {
  const { stdout } = await execFileAsync('ps', ['-axo', 'pid=,ppid=,command=']);
  return stdout.split('\n').flatMap((line) => {
    const match = line.trim().match(/^(\d+)\s+(\d+)\s+(.+)$/);
    return match ? [{ pid: Number(match[1]), parentPid: Number(match[2]), command: match[3] }] : [];
  });
}

async function serviceInfo() {
  const response = await fetch(`http://127.0.0.1:${port}/api/system/info`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

async function shutdownIfRunning() {
  try {
    await fetch(`http://127.0.0.1:${port}/api/system/shutdown`, {
      method: 'POST',
      headers: { 'X-iMail-Daemon-Token': token },
    });
  } catch { /* The service may already be gone. */ }
}

function xmlEscape(value) {
  return value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&apos;');
}

async function writePrivate(target, contents) {
  await writeFile(target, contents);
  await chmod(target, 0o600);
}

const { stdout: uidOutput } = await execFileAsync('id', ['-u']);
const domain = `gui/${uidOutput.trim()}`;

try {
  if (await exists(serviceRoot)) throw new Error(`当前用户已有本地服务目录，拒绝覆盖：${serviceRoot}`);
  if (await exists(plist)) throw new Error(`当前用户已有 iMail LaunchAgent，拒绝覆盖：${plist}`);
  try {
    await serviceInfo();
    throw new Error(`本地端口 ${port} 已有服务，拒绝运行 LaunchAgent 冒烟`);
  } catch (error) {
    if (error instanceof Error && error.message.includes('拒绝运行')) throw error;
  }

  await Promise.all([
    mkdir(runtime, { recursive: true }),
    mkdir(dataDir, { recursive: true }),
    mkdir(logs, { recursive: true }),
    mkdir(launchAgents, { recursive: true }),
  ]);
  createdRoot = true;
  await copyFile(bundledService, serviceExecutable);
  await chmod(serviceExecutable, 0o700);
  await execFileAsync('codesign', ['--verify', '--strict', '--verbose=2', serviceExecutable]);
  await Promise.all([
    writePrivate(path.join(dataDir, 'instance-id'), `${instanceId}\n`),
    writePrivate(controlFile, token),
    writePrivate(enabledFile, 'enabled\n'),
  ]);
  await writePrivate(configPath, `${JSON.stringify({
    serviceExecutable,
    dataDir,
    controlFile,
    enabledFile,
    logFile: path.join(logs, 'service.log'),
    instanceId,
    supervisorId,
    supervisorLockFile,
    host: '127.0.0.1',
    port,
  }, null, 2)}\n`);
  await writePrivate(plist, `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.cooliang.imail.service</string>
<key>ProgramArguments</key><array><string>${xmlEscape(desktop)}</string><string>--imail-daemon</string><string>${xmlEscape(configPath)}</string></array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
</dict></plist>
`);
  createdPlist = true;
  await execFileAsync('plutil', ['-lint', plist]);
  await execFileAsync('launchctl', ['bootstrap', domain, plist]);
  bootstrapped = true;

  const firstInfo = await waitFor(serviceInfo, 'LaunchAgent 首次启动');
  if (firstInfo.service !== 'imail' || firstInfo.instanceId !== instanceId) {
    throw new Error(`LaunchAgent 服务身份异常：${JSON.stringify(firstInfo)}`);
  }
  const firstApi = await waitFor(async () => (await processRows()).find((row) => row.command.includes(serviceExecutable) && row.command.includes(`--port ${port}`)), 'LaunchAgent API 进程');
  const firstWorker = await waitFor(async () => (await processRows()).find((row) => row.parentPid === firstApi.pid && row.command.includes('--sync-worker')), 'LaunchAgent Worker 进程');
  const supervisor = await waitFor(async () => (await processRows()).find((row) => row.command.includes(desktop) && row.command.includes(configPath)), 'LaunchAgent supervisor 进程');

  process.kill(firstApi.pid, 'SIGKILL');
  const recoveredApi = await waitFor(async () => (await processRows()).find((row) => row.pid !== firstApi.pid && row.command.includes(serviceExecutable) && row.command.includes(`--port ${port}`)), 'LaunchAgent API 崩溃恢复');
  await waitFor(async () => !(await processRows()).some((row) => row.pid === firstWorker.pid), '旧 Worker 退出');
  const recoveredWorker = await waitFor(async () => (await processRows()).find((row) => row.parentPid === recoveredApi.pid && row.command.includes('--sync-worker')), '恢复后的 Worker');
  const recoveredInfo = await waitFor(serviceInfo, '恢复后的服务身份');
  if (recoveredInfo.instanceId !== instanceId) throw new Error('LaunchAgent 恢复后实例身份变化');

  await rm(enabledFile, { force: true });
  await execFileAsync('launchctl', ['bootout', domain, plist]);
  bootstrapped = false;
  await shutdownIfRunning();
  await waitFor(async () => {
    try { await serviceInfo(); return false; } catch { return true; }
  }, 'LaunchAgent API 停止');
  await waitFor(async () => {
    const rows = await processRows();
    return !rows.some((row) => [supervisor.pid, recoveredApi.pid, recoveredWorker.pid].includes(row.pid));
  }, 'LaunchAgent 进程树清理');

  console.log(JSON.stringify({
    ok: true,
    launchAgentBootstrap: true,
    signedSupervisor: true,
    signedService: true,
    workerStarted: true,
    crashRecovery: true,
    bootout: true,
    processTreeCleaned: true,
  }));
} catch (reason) {
  const error = reason instanceof Error ? reason : new Error(String(reason));
  const log = await readFile(path.join(logs, 'service.log'), 'utf8').catch(() => '');
  if (log) error.message += `\nserviceLog=${log}`;
  throw error;
} finally {
  await rm(enabledFile, { force: true }).catch(() => undefined);
  if (bootstrapped) await execFileAsync('launchctl', ['bootout', domain, plist]).catch(() => undefined);
  await shutdownIfRunning();
  for (const row of await processRows().catch(() => [])) {
    if (row.command.includes(serviceExecutable) || (row.command.includes(desktop) && row.command.includes(configPath))) {
      try { process.kill(row.pid, 'SIGKILL'); } catch { /* Already stopped. */ }
    }
  }
  await delay(250);
  if (createdPlist) await rm(plist, { force: true });
  if (createdRoot) await rm(serviceRoot, { recursive: true, force: true });
}
