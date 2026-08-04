import { spawnSync } from 'node:child_process';

const buildScript = process.platform === 'win32'
  ? 'build:desktop:windows'
  : process.platform === 'darwin'
    ? 'build:desktop:macos'
    : null;

if (!buildScript) {
  console.error('内部桌面测试包目前只支持在 Windows 或 macOS 构建机上生成。');
  process.exit(1);
}

const npmCli = process.env.npm_execpath;
if (!npmCli) {
  console.error('请通过 npm run build:desktop:internal 启动内部测试构建。');
  process.exit(1);
}

const environment = { ...process.env };
if (process.platform === 'darwin') {
  environment.APPLE_SIGNING_IDENTITY = '-';
}

console.log(`正在生成 ${process.platform === 'win32' ? 'Windows NSIS' : 'macOS ad-hoc DMG'} 内部测试包……`);
const result = spawnSync(process.execPath, [npmCli, 'run', buildScript], {
  env: environment,
  stdio: 'inherit',
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
