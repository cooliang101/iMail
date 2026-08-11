import { execFile } from 'node:child_process';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const options = parseArguments(process.argv.slice(2));
const suffix = `${process.pid}-${Date.now()}`;
const rustImage = `imail-rust-smoke:${suffix}`;
const container = `imail-rust-smoke-${suffix}`;
const dataVolume = `imail-rust-smoke-data-${suffix}`;
const backupVolume = `imail-rust-smoke-backups-${suffix}`;
let rustImageCreated = false;
let containerCreated = false;
let dataVolumeCreated = false;
let backupVolumeCreated = false;
let dockerServerVersion;

export function parseArguments(values) {
  let report;
  let wslDistro;
  let buildEngine = 'docker';
  for (let index = 0; index < values.length; index += 1) {
    if (values[index] === '--report') {
      report = values[++index];
      if (!report) throw new Error('--report 需要非空路径');
    } else if (values[index] === '--wsl-distro') {
      wslDistro = values[++index];
      if (!wslDistro) throw new Error('--wsl-distro 需要发行版名称');
    }
    else if (values[index] === '--build-engine') buildEngine = values[++index];
    else throw new Error(`未知参数：${values[index]}`);
  }
  if (wslDistro !== undefined && (!wslDistro || !/^[\w.-]{1,80}$/.test(wslDistro))) {
    throw new Error('--wsl-distro 只能包含字母、数字、点、下划线和连字符');
  }
  if (!['docker', 'buildx'].includes(buildEngine)) {
    throw new Error('--build-engine 必须为 docker 或 buildx');
  }
  return { report, wslDistro, buildEngine };
}

async function docker(args) {
  const executable = options.wslDistro ? 'wsl.exe' : 'docker';
  const commandArguments = options.wslDistro
    ? ['--distribution', options.wslDistro, '--cd', process.cwd(), '--exec', 'docker', ...args]
    : args;
  return execFileAsync(executable, commandArguments, {
    cwd: process.cwd(),
    windowsHide: true,
    maxBuffer: 20 * 1024 * 1024,
  });
}

async function buildImage(args) {
  return options.buildEngine === 'buildx'
    ? docker(['buildx', 'build', '--quiet', '--load', ...args])
    : docker(['build', ...args]);
}

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function pullImageWithRetry(image) {
  let lastError;
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    try {
      await docker(['pull', image]);
      return;
    } catch (error) {
      lastError = error;
      if (attempt < 3) await delay(attempt * 2_000);
    }
  }
  throw new Error(`拉取容器基础镜像失败：${image}\n${lastError instanceof Error ? lastError.message : String(lastError)}`);
}

async function startContainer(image, runtime) {
  const environment = runtime === 'rust'
    ? ['--env', 'IMAIL_GATEWAY=true', '--env', 'IMAIL_MCP=true', '--env', 'IMAIL_REGISTRATION_OPEN=true']
    : ['--env', 'IMAIL_SYNC_WORKER_MODE=disabled'];
  await docker([
    'run', '--detach', '--name', container,
    '--read-only', '--tmpfs', '/tmp:rw,noexec,nosuid,size=64m',
    '--mount', `type=volume,source=${dataVolume},destination=/data`,
    '--mount', `type=volume,source=${backupVolume},destination=/backups`,
    '--publish', '127.0.0.1::8787',
    '--health-interval', '1s', '--health-timeout', '3s', '--health-start-period', '1s', '--health-retries', '10',
    '--env', 'IMAIL_ALLOWED_HOSTS=127.0.0.1',
    ...environment,
    image,
  ]);
  containerCreated = true;
  const { stdout } = await docker(['port', container, '8787/tcp']);
  const match = stdout.trim().match(/:(\d+)$/);
  if (!match) throw new Error(`无法解析 Rust 容器映射端口：${stdout}`);
  return Number(match[1]);
}

async function waitForInfo(port) {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/api/system/info`);
      if (response.ok) return response.json();
    } catch {
      // Image startup may still be creating or reopening the isolated schema.
    }
    await delay(500);
  }
  const logs = await docker(['logs', container]).then(({ stdout, stderr }) => `${stdout}\n${stderr}`).catch(() => '');
  throw new Error(`Rust 容器未就绪\n${logs}`);
}

async function waitForHealthy() {
  for (let attempt = 0; attempt < 30; attempt += 1) {
    const inspection = JSON.parse((await docker(['inspect', container])).stdout)[0];
    const status = inspection.State?.Health?.Status;
    if (status === 'healthy') return inspection;
    if (status === 'unhealthy') {
      throw new Error(`Rust 容器 healthcheck 失败：${JSON.stringify(inspection.State.Health.Log ?? [])}`);
    }
    await delay(500);
  }
  throw new Error('Rust 容器 healthcheck 未在限定时间内进入 healthy');
}

async function stopAndRemoveContainer(runtime) {
  await docker(['stop', '--time', '10', container]);
  const inspection = JSON.parse((await docker(['inspect', container])).stdout)[0];
  if (inspection.State.ExitCode !== 0) {
    throw new Error(`${runtime} 容器 SIGTERM 后退出码不是 0：${inspection.State.ExitCode}`);
  }
  const stoppedLogs = await docker(['logs', container]).then(({ stdout, stderr }) => `${stdout}\n${stderr}`);
  if (runtime === 'rust' && !stoppedLogs.includes('stopped gracefully')) {
    throw new Error(`Rust 容器未在 SIGTERM 后优雅关闭\n${stoppedLogs}`);
  }
  await docker(['rm', container]);
  containerCreated = false;
}

try {
  dockerServerVersion = (await docker(['version', '--format', '{{.Server.Version}}'])).stdout.trim();
  if (!dockerServerVersion) {
    throw new Error('Docker daemon 没有返回服务端版本');
  }
  await docker(['info', '--format', '{{.OSType}}/{{.Architecture}}']).then(({ stdout }) => {
    if (stdout.trim() !== 'linux/x86_64') {
      throw new Error(`Docker daemon 平台不是 linux/x86_64：${stdout.trim()}`);
    }
  }).catch((error) => {
    if (error instanceof Error && error.message.includes('平台不是')) throw error;
    throw new Error('Docker daemon 不可用；请在安装并启动 Docker 的发布机或 WSL2 中运行 Rust 容器验收');
  });
  if (options.wslDistro) {
    await pullImageWithRetry('node:24-bookworm-slim');
    await pullImageWithRetry('rust:1.77.2-bookworm');
    await pullImageWithRetry('debian:bookworm-slim');
  }
  if (options.buildEngine === 'buildx') {
    await docker(['buildx', 'version']);
  }
  await buildImage([
    '--platform', 'linux/amd64', '--file', 'Dockerfile', '--tag', rustImage, '.',
  ]);
  rustImageCreated = true;
  const imageInspection = JSON.parse((await docker(['image', 'inspect', rustImage])).stdout)[0];
  if (imageInspection.Os !== 'linux' || imageInspection.Architecture !== 'amd64') {
    throw new Error(`Rust 镜像平台错误：${imageInspection.Os}/${imageInspection.Architecture}`);
  }
  await docker(['volume', 'create', dataVolume]);
  dataVolumeCreated = true;
  await docker(['volume', 'create', backupVolume]);
  backupVolumeCreated = true;

  const seedPort = await startContainer(rustImage, 'rust');
  const seedInfo = await waitForInfo(seedPort);
  if (!seedInfo?.instanceId || !seedInfo?.capabilities?.webClient) {
    throw new Error(`Rust 种子容器 capability 不完整：${JSON.stringify(seedInfo)}`);
  }
  await waitForHealthy();
  const registration = await fetch(`http://127.0.0.1:${seedPort}/api/auth/register`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      login: 'container-persistence-owner',
      displayName: 'Container Persistence Owner',
      password: 'Container persistence password 123!',
    }),
  });
  if (registration.status !== 201) {
    throw new Error(`Rust 种子容器初始化注册失败：${registration.status} ${await registration.text()}`);
  }
  const seedCookie = registration.headers.get('set-cookie')?.split(';', 1)[0];
  if (!seedCookie) throw new Error('Rust 种子容器注册没有返回 Session Cookie');
  const preferences = await fetch(`http://127.0.0.1:${seedPort}/api/preferences`, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json', cookie: seedCookie },
    body: JSON.stringify({ theme: 'tech', startupView: 'starred' }),
  });
  if (!preferences.ok) {
    throw new Error(`Rust 种子容器偏好写入失败：${preferences.status} ${await preferences.text()}`);
  }
  const externalAccess = await fetch(`http://127.0.0.1:${seedPort}/api/external-access`, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json', cookie: seedCookie },
    body: JSON.stringify({ gatewayEnabled: true, mcpEnabled: true }),
  });
  if (!externalAccess.ok) {
    throw new Error(`Rust 种子容器外部访问设置写入失败：${externalAccess.status} ${await externalAccess.text()}`);
  }
  await stopAndRemoveContainer('rust');

  const firstPort = await startContainer(rustImage, 'rust');
  const firstInfo = await waitForInfo(firstPort);
  if (!firstInfo?.instanceId || !firstInfo?.capabilities?.webClient || !firstInfo.capabilities.gateway
    || !firstInfo.capabilities.mcp || !firstInfo.capabilities.syncWorker) {
    throw new Error(`Rust 容器 capability 不完整：${JSON.stringify(firstInfo)}`);
  }
  if (firstInfo.instanceId !== seedInfo.instanceId) {
    throw new Error(`Rust 容器重启后实例身份变化：${seedInfo.instanceId} -> ${firstInfo.instanceId}`);
  }
  const firstInspection = await waitForHealthy();
  if (firstInspection.Config.User !== '10001:10001') {
    throw new Error(`Rust 容器未使用固定非 root 用户：${firstInspection.Config.User || 'root'}`);
  }
  if (!firstInspection.HostConfig.ReadonlyRootfs) throw new Error('Rust 容器未通过只读根文件系统运行');
  if (!firstInspection.Config.Healthcheck) throw new Error('Rust 镜像缺少 healthcheck');
  const mountNames = new Map(firstInspection.Mounts.map((mount) => [mount.Destination, mount.Name]));
  if (mountNames.get('/data') !== dataVolume || mountNames.get('/backups') !== backupVolume) {
    throw new Error(`Rust 容器没有使用预期的持久卷：${JSON.stringify(firstInspection.Mounts)}`);
  }
  const page = await fetch(`http://127.0.0.1:${firstPort}/`);
  if (!page.ok || !(await page.text()).includes('<div id="root"></div>')) {
    throw new Error('Rust 容器 Web 应用外壳验证失败');
  }
  const login = await fetch(`http://127.0.0.1:${firstPort}/api/auth/login`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      login: 'container-persistence-owner',
      password: 'Container persistence password 123!',
    }),
  });
  if (!login.ok || !login.headers.get('set-cookie')) {
    throw new Error(`Rust 未能读取重启前的登录数据：${login.status} ${await login.text()}`);
  }
  const rustCookie = login.headers.get('set-cookie').split(';', 1)[0];
  const migratedPreferencesResponse = await fetch(`http://127.0.0.1:${firstPort}/api/preferences`, {
    headers: { cookie: rustCookie },
  });
  const migratedPreferences = await migratedPreferencesResponse.json();
  if (!migratedPreferencesResponse.ok || migratedPreferences.preferences?.theme !== 'tech'
    || migratedPreferences.preferences?.startupView !== 'starred') {
    throw new Error(`Rust 未能读取重启前的偏好：${JSON.stringify(migratedPreferences)}`);
  }
  const migratedAccessResponse = await fetch(`http://127.0.0.1:${firstPort}/api/external-access`, {
    headers: { cookie: rustCookie },
  });
  const migratedAccess = await migratedAccessResponse.json();
  if (!migratedAccessResponse.ok || !migratedAccess.settings?.gatewayEnabled
    || !migratedAccess.settings?.mcpEnabled) {
    throw new Error(`Rust 未能读取重启前的外部访问设置：${JSON.stringify(migratedAccess)}`);
  }
  await stopAndRemoveContainer('rust');

  const secondPort = await startContainer(rustImage, 'rust');
  const secondInfo = await waitForInfo(secondPort);
  await waitForHealthy();
  if (secondInfo.instanceId !== firstInfo.instanceId) {
    throw new Error(`Rust 容器重启后实例身份变化：${firstInfo.instanceId} -> ${secondInfo.instanceId}`);
  }
  const authStatusResponse = await fetch(`http://127.0.0.1:${secondPort}/api/auth/status`);
  const authStatus = await authStatusResponse.json();
  if (!authStatusResponse.ok || authStatus.setupRequired !== false) {
    throw new Error(`Rust 容器重启后没有保留初始化数据：${JSON.stringify(authStatus)}`);
  }
  const backup = JSON.parse((await docker([
    'exec', container, '/app/imail-maintenance', 'backup', '/backups/smoke-backup',
  ])).stdout);
  if (!backup.ok || backup.schemaVersion !== 6 || backup.fileCount < 3) {
    throw new Error(`Rust 容器备份失败：${JSON.stringify(backup)}`);
  }
  const restore = JSON.parse((await docker([
    'exec', container, '/app/imail-maintenance', 'restore', '/backups/smoke-backup', '/backups/smoke-restore',
  ])).stdout);
  if (!restore.ok || !restore.databaseVerified || !restore.integrityManifestVerified || restore.schemaVersion !== 6) {
    throw new Error(`Rust 容器恢复预检失败：${JSON.stringify(restore)}`);
  }
  let duplicateRestoreRejected = false;
  try {
    await docker([
      'exec', container, '/app/imail-maintenance', 'restore', '/backups/smoke-backup', '/backups/smoke-restore',
    ]);
  } catch {
    duplicateRestoreRejected = true;
  }
  if (!duplicateRestoreRejected) throw new Error('Rust 容器恢复覆盖了已存在的目标目录');
  const preflight = JSON.parse((await docker([
    'exec', container, '/app/imail-maintenance', 'upgrade-preflight', '/backups/preflight-backup', '/backups/preflight-copy',
  ])).stdout);
  if (!preflight.ok || !preflight.activeDataUntouched || !preflight.integrityManifestVerified
    || !preflight.sqliteQuickCheck || !preflight.foreignKeysVerified || preflight.migratedSchemaVersion !== 6) {
    throw new Error(`Rust 容器升级预检失败：${JSON.stringify(preflight)}`);
  }
  await stopAndRemoveContainer('rust');
  const result = {
    schemaVersion: 1,
    ok: true,
    generatedAt: new Date().toISOString(),
    dockerServerVersion,
    dockerTransport: options.wslDistro ? `wsl:${options.wslDistro}` : 'native',
    buildEngine: options.buildEngine,
    image: rustImage,
    imageId: imageInspection.Id,
    platform: 'linux/amd64',
    webClient: true,
    gateway: true,
    mcp: true,
    syncWorker: true,
    nonRootUser: true,
    readOnlyRootFilesystem: true,
    healthcheckHealthy: true,
    persistentVolumeRestart: true,
    rustVolumePersistence: true,
    preferencesPreserved: true,
    externalAccessPreserved: true,
    instanceIdentityPreserved: true,
    initializedDataPreserved: true,
    backupRestore: true,
    nonOverwritingRestore: true,
    upgradePreflight: true,
    gracefulSigterm: true,
  };
  const serialized = `${JSON.stringify(result, null, 2)}\n`;
  if (options.report) {
    await writeFile(path.resolve(options.report), serialized, { flag: 'wx' });
  }
  process.stdout.write(serialized);
} finally {
  if (containerCreated) await docker(['rm', '--force', container]).catch(() => undefined);
  if (rustImageCreated) await docker(['image', 'rm', '--force', rustImage]).catch(() => undefined);
  if (dataVolumeCreated) await docker(['volume', 'rm', '--force', dataVolume]).catch(() => undefined);
  if (backupVolumeCreated) await docker(['volume', 'rm', '--force', backupVolume]).catch(() => undefined);
}
