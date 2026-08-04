import { execFile, execFileSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { access, copyFile, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { promisify } from 'node:util';
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';

const execFileAsync = promisify(execFile);
const root = path.resolve(import.meta.dirname, '..');
async function availablePort() {
  if (process.env.IMAIL_DAEMON_SMOKE_PORT) return Number(process.env.IMAIL_DAEMON_SMOKE_PORT);
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      const selected = typeof address === 'object' && address ? address.port : 0;
      server.close((error) => error ? reject(error) : resolve(selected));
    });
  });
}

const port = await availablePort();
if (!Number.isInteger(port) || port < 1 || port > 65_535) throw new Error('守护冒烟端口无效');
const windowsTarget = `${process.arch === 'arm64' ? 'aarch64' : 'x86_64'}-pc-windows-msvc`;
const manager = path.join(root, 'src-tauri', 'cleanup-helper', 'target', windowsTarget, 'debug', 'imail-service-manager.exe');
const binaryDirectory = path.join(root, 'src-tauri', 'binaries');

if (process.platform !== 'win32') throw new Error('当前冒烟脚本验证 Windows 用户级 supervisor；macOS 使用发布矩阵中的 LaunchAgent 验收');
await access(manager).catch(() => { throw new Error('缺少 debug 服务管理程序，请先执行 npm run build:service-runtime'); });
const serviceName = (await readdir(binaryDirectory)).find((name) => /^imail-service-(?!manager-).*\.exe$/.test(name));
if (!serviceName) throw new Error('缺少服务 sidecar，请先执行 npm run build:service-runtime');
const serviceSource = path.join(binaryDirectory, serviceName);
const temporaryBase = await mkdtemp(path.join(tmpdir(), 'imail-daemon-smoke-'));
const temporaryRoot = path.join(temporaryBase, 'com.cooliang.imail', 'local-service');
const serviceExecutable = path.join(temporaryRoot, 'runtime', 'imail-service-smoke.exe');
const dataDir = path.join(temporaryRoot, 'data');
const logsDir = path.join(temporaryRoot, 'logs');
const configPath = path.join(temporaryRoot, 'daemon.json');
const controlFile = path.join(temporaryRoot, 'control-token');
const enabledFile = path.join(temporaryRoot, 'enabled');
const instanceId = randomUUID();
let supervisor;
let duplicateSupervisor;
let failureSupervisor;
let supervisorError = '';
let cleanupCompleted = false;

process.once('exit', () => {
  if (cleanupCompleted || !supervisor?.pid || supervisor.exitCode !== null) return;
  try {
    execFileSync('taskkill.exe', ['/PID', String(supervisor.pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true });
  } catch { /* The process tree may already be gone. */ }
});

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function waitFor(check, description, timeoutMs = 20_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try { const result = await check(); if (result) return result; }
    catch (error) { lastError = error; }
    await delay(250);
  }
  throw new Error(`${description}超时${lastError ? `：${lastError}` : ''}`);
}

async function serviceInfo() {
  if (supervisor?.exitCode !== null) throw new Error(`服务管理程序已退出（${supervisor.exitCode}）：${supervisorError.trim()}`);
  const response = await fetch(`http://127.0.0.1:${port}/api/system/info`);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

async function ownedProcesses() {
  const script = "Get-CimInstance Win32_Process -Filter \"Name LIKE 'imail-service%.exe'\" | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine | ConvertTo-Json -Compress";
  const { stdout } = await execFileAsync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { windowsHide: true });
  if (!stdout.trim()) return [];
  const parsed = JSON.parse(stdout);
  const rows = Array.isArray(parsed) ? parsed : [parsed];
  return rows.filter((row) => String(row.ExecutablePath || '').toLowerCase() === serviceExecutable.toLowerCase());
}

async function writeConfig(supervisorId) {
  await writeFile(configPath, `${JSON.stringify({
    serviceExecutable, dataDir, controlFile, enabledFile,
    logFile: path.join(logsDir, 'service.log'), instanceId, supervisorId,
    host: '127.0.0.1', port,
  }, null, 2)}\n`);
}

try {
  await Promise.all([mkdir(dataDir, { recursive: true }), mkdir(logsDir, { recursive: true }), mkdir(path.join(temporaryRoot, 'runtime'), { recursive: true })]);
  await Promise.all([
    copyFile(serviceSource, serviceExecutable),
    writeFile(path.join(dataDir, 'instance-id'), `${instanceId}\n`),
    writeFile(controlFile, `${randomUUID().replaceAll('-', '')}${randomUUID().replaceAll('-', '')}`),
    writeFile(enabledFile, 'enabled\n'),
  ]);
  await writeConfig(randomUUID());
  const supervisorEnvironment = { ...process.env, IMAIL_SMOKE_LOCAL_APP_DATA: temporaryBase };
  supervisor = spawn(manager, ['--imail-daemon', configPath], { cwd: root, stdio: ['ignore', 'ignore', 'pipe'], windowsHide: true, env: supervisorEnvironment });
  supervisor.stderr.on('data', (chunk) => { supervisorError += chunk.toString(); });

  let firstInfo;
  try {
    firstInfo = await waitFor(serviceInfo, '首次启动');
  } catch (error) {
    const serviceLog = await readFile(path.join(logsDir, 'service.log'), 'utf8').catch(() => '');
    const diagnostic = await readFile(path.join(temporaryRoot, 'supervisor-status.json'), 'utf8').catch(() => '');
    throw new Error(`${error.message}\nmanager=${supervisorError.trim()}\ndiagnostic=${diagnostic}\nserviceLog=${serviceLog}`);
  }
  if (firstInfo.instanceId !== instanceId) throw new Error('服务实例身份与守护配置不一致');
  const firstMain = await waitFor(async () => (await ownedProcesses()).find((row) => String(row.CommandLine).includes(`--port ${port}`)), '查找首次 API 进程');
  const firstWorker = await waitFor(async () => (await ownedProcesses()).find((row) => row.ParentProcessId === firstMain.ProcessId && String(row.CommandLine).includes('--sync-worker')), '查找首次同步 Worker');
  duplicateSupervisor = spawn(manager, ['--imail-daemon', configPath], { cwd: root, stdio: 'ignore', windowsHide: true, env: supervisorEnvironment });
  await waitFor(async () => duplicateSupervisor.exitCode !== null, '重复 supervisor 自退', 5_000);
  const afterDuplicate = await ownedProcesses();
  if (afterDuplicate.filter((row) => String(row.CommandLine).includes(`--port ${port}`)).length !== 1) throw new Error('supervisor 锁未阻止重复 API 进程');

  process.kill(firstMain.ProcessId, 'SIGTERM');
  const secondMain = await waitFor(async () => (await ownedProcesses()).find((row) => row.ProcessId !== firstMain.ProcessId && String(row.CommandLine).includes(`--port ${port}`)), 'API 崩溃恢复');
  await waitFor(async () => !(await ownedProcesses()).some((row) => row.ProcessId === firstWorker.ProcessId), '孤儿 Worker 退出');
  const secondWorker = await waitFor(async () => (await ownedProcesses()).find((row) => row.ParentProcessId === secondMain.ProcessId && String(row.CommandLine).includes('--sync-worker')), '恢复后的同步 Worker');
  const recoveredInfo = await waitFor(serviceInfo, '恢复后的身份检查');
  if (recoveredInfo.instanceId !== instanceId) throw new Error('恢复后服务实例身份发生变化');

  await writeConfig(randomUUID());
  await waitFor(async () => supervisor.exitCode !== null, '旧代 supervisor 退出');
  await waitFor(async () => {
    try { await serviceInfo(); return false; } catch { return true; }
  }, '旧代服务停止');
  await waitFor(async () => !(await ownedProcesses()).some((row) => row.ProcessId === secondWorker.ProcessId), '旧代 Worker 退出');

  const cleanup = spawn(manager, ['--imail-uninstall-cleanup'], {
    cwd: root, stdio: 'ignore', windowsHide: true,
    env: { ...process.env, IMAIL_SMOKE_LOCAL_APP_DATA: temporaryBase, IMAIL_SMOKE_SKIP_STARTUP_REGISTRATION: 'true' },
  });
  const cleanupCode = await new Promise((resolve) => cleanup.once('exit', resolve));
  if (cleanupCode !== 0) throw new Error(`卸载清理退出码异常：${cleanupCode}`);
  await access(dataDir);
  const mutableRuntimeRemoved = await access(configPath).then(() => false, () => true)
    && await access(path.join(temporaryRoot, 'runtime')).then(() => false, () => true);
  if (!mutableRuntimeRemoved) throw new Error('卸载清理未删除守护配置或运行目录');

  await mkdir(logsDir, { recursive: true });
  await Promise.all([writeFile(enabledFile, 'enabled\n'), writeFile(controlFile, 'invalid-test-token')]);
  await writeFile(configPath, `${JSON.stringify({
    serviceExecutable: serviceSource, dataDir, controlFile, enabledFile,
    logFile: path.join(logsDir, 'service.log'), instanceId, supervisorId: randomUUID(),
    host: '127.0.0.1', port: port + 1,
  }, null, 2)}\n`);
  const rejectedConfigSupervisor = spawn(manager, ['--imail-daemon', configPath], { cwd: root, stdio: 'ignore', windowsHide: true, env: supervisorEnvironment });
  await waitFor(async () => rejectedConfigSupervisor.exitCode !== null, '越界守护配置拒绝', 5_000);
  let unsafeServiceStarted = false;
  try { await fetch(`http://127.0.0.1:${port + 1}/api/system/info`); unsafeServiceStarted = true; } catch { /* Expected. */ }
  if (unsafeServiceStarted) throw new Error('越界守护配置启动了服务进程');

  await writeConfig(randomUUID());
  const diagnosticFile = path.join(temporaryRoot, 'supervisor-status.json');
  failureSupervisor = spawn(manager, ['--imail-daemon', configPath], { cwd: root, stdio: 'ignore', windowsHide: true, env: supervisorEnvironment });
  const diagnostic = await waitFor(async () => JSON.parse(await readFile(diagnosticFile, 'utf8')), '持久化 supervisor 失败诊断');
  if (diagnostic.reason !== 'serviceSpawnFailed' || diagnostic.failures < 1) throw new Error('supervisor 失败诊断内容无效');
  await delay(1_000);
  if (failureSupervisor.exitCode !== null) throw new Error('服务启动失败后 supervisor 未保持退避守护');
  await rm(enabledFile, { force: true });
  await waitFor(async () => failureSupervisor.exitCode !== null, '失败 supervisor 停用退出', 5_000);

  console.log(JSON.stringify({
    ok: true, port, instanceId,
    firstApiPid: firstMain.ProcessId, recoveredApiPid: secondMain.ProcessId,
    firstWorkerPid: firstWorker.ProcessId, recoveredWorkerPid: secondWorker.ProcessId,
    duplicateSupervisorPrevented: true, generationRotationStoppedOldSupervisor: true,
    uninstallCleanupRemovedRuntime: true, uninstallCleanupPreservedData: true,
    unmanagedConfigRejected: true, persistentFailureDiagnostic: true,
  }));
} finally {
  await rm(enabledFile, { force: true }).catch(() => undefined);
  if (supervisor && supervisor.exitCode === null) supervisor.kill('SIGTERM');
  if (duplicateSupervisor && duplicateSupervisor.exitCode === null) duplicateSupervisor.kill('SIGTERM');
  if (failureSupervisor && failureSupervisor.exitCode === null) failureSupervisor.kill('SIGTERM');
  for (const owned of await ownedProcesses().catch(() => [])) {
    try { process.kill(owned.ProcessId, 'SIGTERM'); } catch { /* Already stopped. */ }
  }
  await delay(250);
  await rm(temporaryBase, { recursive: true, force: true });
  cleanupCompleted = true;
}
