import { execFile } from 'node:child_process';
import { access, mkdir, readdir, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const root = path.resolve(import.meta.dirname, '..');

if (process.platform !== 'win32') throw new Error('NSIS 安装冒烟只能在 Windows 发布机运行');
if (process.env.CI !== 'true' && process.env.IMAIL_ALLOW_INSTALLER_SMOKE !== 'true') {
  throw new Error('为避免改写现有用户安装，此脚本只在 CI 或显式设置 IMAIL_ALLOW_INSTALLER_SMOKE=true 时运行');
}

const cargoTargetDir = process.env.CARGO_TARGET_DIR || path.join(root, 'src-tauri', 'target');
const bundleDir = path.join(cargoTargetDir, 'x86_64-pc-windows-msvc', 'release', 'bundle', 'nsis');
const installerName = (await readdir(bundleDir)).find((name) => name.endsWith('-setup.exe'));
if (!installerName) throw new Error('缺少 NSIS 安装包，请先执行 npm --prefix frontend run build:desktop:windows');

const installDir = path.join(process.env.LOCALAPPDATA || '', 'iMail');
const localServiceRoot = path.join(process.env.LOCALAPPDATA || '', 'com.cooliang.imail', 'local-service');
const dataMarker = path.join(localServiceRoot, 'data', 'preserved-by-uninstall.txt');
const runtimeMarker = path.join(localServiceRoot, 'runtime', 'removed-by-uninstall.txt');
const uninstallRegistryKey = String.raw`HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\iMail`;
const startupRegistryKey = String.raw`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`;
const startupRegistryValue = 'iMailService';
const desktopLaunchTimeoutMs = 120_000;
const environment = { ...process.env };
let uninstaller;
let uninstalled = false;
let createdLocalServiceRoot = false;
let createdStartupRegistryValue = false;

async function exists(target) {
  return access(target).then(() => true, () => false);
}

async function registryExists() {
  return execFileAsync('reg.exe', ['query', uninstallRegistryKey], { windowsHide: true })
    .then(() => true, () => false);
}

async function startupRegistryValueExists() {
  return execFileAsync('reg.exe', ['query', startupRegistryKey, '/v', startupRegistryValue], { windowsHide: true })
    .then(() => true, () => false);
}

async function waitUntilRemoved(target, timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!await exists(target)) return;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`等待删除超时：${target}`);
}

async function waitUntilRegistryRemoved(timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!await registryExists()) return;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error('NSIS 卸载后当前用户卸载注册仍然存在');
}

async function waitForInstalledFiles(timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const names = await readdir(installDir);
      const required = ['imail.exe'];
      const sizes = await Promise.all(required.map(async (requiredName) => {
        const actual = names.find((name) => name.toLowerCase() === requiredName);
        return actual ? (await stat(path.join(installDir, actual))).size : 0;
      }));
      if (sizes.every((size) => size > 0) && names.some((name) => /^uninstall.*\.exe$/i.test(name))) return names;
    } catch { /* The NSIS child process is still installing. */ }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error('NSIS 安装文件在等待时间内未完整写入');
}

try {
  if (!process.env.LOCALAPPDATA) throw new Error('无法确定当前用户 LOCALAPPDATA');
  if (await exists(installDir) || await registryExists()) throw new Error('当前用户已安装 iMail，拒绝覆盖现有安装');
  if (await exists(localServiceRoot)) throw new Error('当前用户已有 iMail 本地服务数据，拒绝运行安装卸载冒烟');
  if (await startupRegistryValueExists()) throw new Error('当前用户已有 iMail 本地服务自启动项，拒绝运行安装卸载冒烟');

  await execFileAsync(path.join(bundleDir, installerName), ['/S', '/NS'], {
    cwd: root,
    env: environment,
    windowsHide: true,
    timeout: 120_000,
  });

  const installedFiles = await waitForInstalledFiles();
  const desktopName = installedFiles.find((name) => name.toLowerCase() === 'imail.exe');
  const uninstallerName = installedFiles.find((name) => /^uninstall.*\.exe$/i.test(name));
  if (!desktopName || !uninstallerName) {
    throw new Error(`安装目录缺少桌面程序或卸载程序：${installedFiles.join(', ')}`);
  }
  if (installedFiles.some((name) => /^imail-service(?:-manager)?\.exe$/i.test(name))) {
    throw new Error(`Rust-only 安装包不应包含 Node 服务或旧管理程序：${installedFiles.join(', ')}`);
  }
  uninstaller = path.join(installDir, uninstallerName);

  await execFileAsync(path.join(installDir, desktopName), [], {
    cwd: installDir,
    env: { ...environment, IMAIL_DESKTOP_SMOKE_TEST: 'true' },
    windowsHide: true,
    timeout: desktopLaunchTimeoutMs,
  });

  await Promise.all([
    mkdir(path.dirname(dataMarker), { recursive: true }),
    mkdir(path.dirname(runtimeMarker), { recursive: true }),
  ]);
  createdLocalServiceRoot = true;
  await Promise.all([
    writeFile(dataMarker, 'preserve\n'),
    writeFile(runtimeMarker, 'remove\n'),
  ]);
  const startupCommand = `"${path.join(installDir, 'retained-legacy-manager.exe')}" --imail-daemon "${path.join(localServiceRoot, 'daemon.json')}"`;
  await execFileAsync('reg.exe', ['add', startupRegistryKey, '/v', startupRegistryValue, '/t', 'REG_SZ', '/d', startupCommand, '/f'], {
    windowsHide: true,
  });
  createdStartupRegistryValue = true;
  if (!await startupRegistryValueExists()) throw new Error('无法创建受控的用户级守护自启动测试项');

  await execFileAsync(uninstaller, ['/S', '/NS'], {
    cwd: root,
    env: environment,
    windowsHide: true,
    timeout: 120_000,
  });
  await waitUntilRemoved(installDir);
  await waitUntilRegistryRemoved();
  uninstalled = true;

  if (await startupRegistryValueExists()) throw new Error('NSIS 卸载 hook 未删除用户级守护自启动项');
  if (!await exists(dataMarker)) throw new Error('NSIS 卸载错误删除了本地邮件数据');
  if (await exists(runtimeMarker)) throw new Error('NSIS 卸载 hook 未删除本地服务运行文件');

  console.log(JSON.stringify({
    ok: true,
    silentInstall: true,
    rustOnlyDesktop: true,
    bundledSidecar: false,
    bundledServiceManager: false,
    installedAppLaunch: true,
    uninstallHookRan: true,
    uninstallRemovedUserStartup: true,
    uninstallPreservedData: true,
  }));
} finally {
  if (!uninstalled && uninstaller && await exists(uninstaller)) {
    await execFileAsync(uninstaller, ['/S', '/NS'], {
      cwd: root,
      env: environment,
      windowsHide: true,
      timeout: 120_000,
    }).catch(() => undefined);
    await waitUntilRemoved(installDir, 30_000).catch(() => undefined);
  }
  if (createdStartupRegistryValue) {
    await execFileAsync('reg.exe', ['delete', startupRegistryKey, '/v', startupRegistryValue, '/f'], { windowsHide: true })
      .catch(() => undefined);
  }
  if (createdLocalServiceRoot) await rm(localServiceRoot, { recursive: true, force: true });
}
