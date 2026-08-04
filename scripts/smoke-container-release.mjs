import { execFile } from 'node:child_process';
import path from 'node:path';
import process from 'node:process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const suffix = `${process.pid}-${Date.now()}`;
const image = `imail-release-smoke:${suffix}`;
const container = `imail-release-smoke-${suffix}`;
let containerCreated = false;
let imageCreated = false;

async function docker(args, options = {}) {
  return execFileAsync('docker', args, { windowsHide: true, maxBuffer: 20 * 1024 * 1024, ...options });
}

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

try {
  await docker(['version', '--format', '{{.Server.Version}}']).catch(() => {
    throw new Error('Docker daemon 不可用；请在安装并启动 Docker 的发布机上运行此验收');
  });
  await docker(['compose', '--env-file', 'deploy/remote.env.example', '-f', 'compose.example.yml', 'config'], { cwd: process.cwd() });
  await docker(['compose', '--env-file', 'deploy/remote.env.example', '-f', 'compose.https.example.yml', 'config'], { cwd: process.cwd() });
  await docker([
    'run', '--rm', '--entrypoint', 'caddy',
    '--env', 'IMAIL_PUBLIC_HOST=mail.example.com',
    '--volume', `${path.resolve('deploy/Caddyfile')}:/etc/caddy/Caddyfile:ro`,
    'caddy:2-alpine', 'validate', '--config', '/etc/caddy/Caddyfile', '--adapter', 'caddyfile',
  ]);
  await docker(['build', '--tag', image, '.'], { cwd: process.cwd() });
  imageCreated = true;
  await docker([
    'run', '--detach', '--rm', '--name', container,
    '--publish', '127.0.0.1::8787',
    '--env', `APP_MASTER_KEY=${'42'.repeat(32)}`,
    '--env', 'IMAIL_ALLOWED_HOSTS=127.0.0.1',
    '--env', 'MCP_ALLOWED_HOSTS=127.0.0.1',
    image,
  ]);
  containerCreated = true;
  const { stdout } = await docker(['port', container, '8787/tcp']);
  const match = stdout.trim().match(/:(\d+)$/);
  if (!match) throw new Error(`无法解析容器映射端口：${stdout}`);
  const port = Number(match[1]);
  let info;
  for (let index = 0; index < 90; index += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/api/system/info`);
      if (response.ok) { info = await response.json(); break; }
    } catch { /* Container is still starting. */ }
    await delay(500);
  }
  if (!info?.capabilities?.webClient) {
    const logs = await docker(['logs', container]).then((result) => `${result.stdout}\n${result.stderr}`).catch(() => '');
    throw new Error(`容器未就绪或未托管 Web 客户端\n${logs}`);
  }
  const page = await fetch(`http://127.0.0.1:${port}/`);
  if (!page.ok || !(await page.text()).includes('<div id="root"></div>')) throw new Error('容器 Web 应用外壳验证失败');
  const backupPath = `/tmp/imail-backup-${suffix}`;
  const preflightPath = `/tmp/imail-preflight-${suffix}`;
  const preflight = JSON.parse((await docker(['exec', container, 'node', 'server-runtime/imail-upgrade-preflight.mjs', backupPath, preflightPath])).stdout);
  if (!preflight.activeDataUntouched || !preflight.integrityManifestVerified || !preflight.sqliteQuickCheck || !preflight.foreignKeysVerified) {
    throw new Error('容器内升级预检工具验证失败');
  }
  const inspection = JSON.parse((await docker(['inspect', container])).stdout)[0];
  if (inspection.Config.User !== 'imail') throw new Error(`容器未使用非 root 用户：${inspection.Config.User || 'root'}`);
  console.log(JSON.stringify({
    ok: true,
    image,
    webClient: true,
    nonRootUser: true,
    healthcheck: Boolean(inspection.Config.Healthcheck),
    composeValidated: true,
    httpsProxyValidated: true,
    upgradePreflight: true,
    maintenanceTools: true,
  }));
} finally {
  if (containerCreated) await docker(['rm', '--force', container]).catch(() => undefined);
  if (imageCreated) await docker(['image', 'rm', '--force', image]).catch(() => undefined);
}
