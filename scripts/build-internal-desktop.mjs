import { spawnSync } from 'node:child_process';

if (process.platform !== 'win32') {
  console.error('当前桌面交付只支持在 Windows 构建机上生成 NSIS 安装包；服务端请使用 Docker。');
  process.exit(1);
}

const buildScript = 'build:desktop:windows';

const npmCli = process.env.npm_execpath;
if (!npmCli) {
  console.error('请通过 npm --prefix frontend run build:desktop:internal 启动内部测试构建。');
  process.exit(1);
}

console.log('正在生成 Windows NSIS 内部测试包……');
const result = spawnSync(process.execPath, [npmCli, 'run', buildScript], {
  env: process.env,
  stdio: 'inherit',
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
