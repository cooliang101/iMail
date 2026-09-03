import { spawn } from 'node:child_process';
import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath, pathToFileURL } from 'node:url';

const workspaceRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const reportRoot = path.join(workspaceRoot, 'output', 'rust-migration-tests');

export function parseArguments(values) {
  if (values.length === 3 && values.every((value) => !value.startsWith('--'))) {
    values = [
      '--duration-seconds', values[0],
      '--max-growth-mib', values[1],
      '--report', values[2],
    ];
  }
  let durationSeconds;
  let maximumGrowthMiB = 32;
  let report;
  for (let index = 0; index < values.length; index += 1) {
    const argument = values[index];
    if (argument === '--duration-seconds') durationSeconds = Number(values[++index]);
    else if (argument === '--max-growth-mib') maximumGrowthMiB = Number(values[++index]);
    else if (argument === '--report') report = values[++index];
    else throw new Error(`未知参数：${argument}；npm 用法为 <秒> <MiB> <报告>`);
  }
  if (!Number.isInteger(durationSeconds) || durationSeconds < 30 || durationSeconds > 86_400) {
    throw new Error('--duration-seconds 必须为 30..86400 的整数');
  }
  if (!Number.isInteger(maximumGrowthMiB) || maximumGrowthMiB <= 0) {
    throw new Error('--max-growth-mib 必须是大于 0 的整数');
  }
  if (!report || path.extname(report).toLowerCase() !== '.json') {
    throw new Error('--report 必须是 output/rust-migration-tests 下的新 .json 文件');
  }
  const reportPath = path.resolve(workspaceRoot, report);
  const relative = path.relative(reportRoot, reportPath);
  if (!relative || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error('--report 必须位于 output/rust-migration-tests 内且不能指向目录本身');
  }
  if (relative.split(path.sep).some((segment) => segment.toLowerCase() === '.data')) {
    throw new Error('--report 不得位于 .data');
  }
  return { durationSeconds, maximumGrowthMiB, reportPath };
}

async function pathExists(target) {
  return access(target).then(() => true, () => false);
}

async function run() {
  const options = parseArguments(process.argv.slice(2));
  if (await pathExists(options.reportPath)) {
    throw new Error('长稳报告目标已存在，拒绝覆盖');
  }
  const parent = path.dirname(options.reportPath);
  if (!await pathExists(parent)) {
    throw new Error('长稳报告父目录不存在；请先确认验收输出目录');
  }
  const cargoTargetArguments = process.platform === 'win32'
    ? ['--target', 'x86_64-pc-windows-msvc']
    : [];
  const args = [
    'test', '--release', ...cargoTargetArguments, '--locked',
    '-p', 'imail-http',
    'tests::real_mail_fixture::real_tls_idle_runtime_soak_acceptance',
    '--', '--ignored', '--exact', '--nocapture',
  ];
  const child = spawn('cargo', args, {
    cwd: workspaceRoot,
    env: {
      ...process.env,
      IMAIL_RUST_TLS_SOAK_ALLOW: 'isolated-loopback',
      IMAIL_RUST_TLS_SOAK_DURATION_SECONDS: String(options.durationSeconds),
      IMAIL_RUST_TLS_SOAK_MAX_GROWTH_MIB: String(options.maximumGrowthMiB),
      IMAIL_RUST_TLS_SOAK_REPORT: options.reportPath,
    },
    stdio: 'inherit',
    windowsHide: true,
  });
  const exitCode = await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', (code, signal) => signal ? reject(new Error(`长稳验收被信号终止：${signal}`)) : resolve(code));
  });
  if (exitCode !== 0) throw new Error(`真实 TLS 长稳验收失败，cargo 退出码 ${exitCode}`);
  const evidence = JSON.parse(await readFile(options.reportPath, 'utf8'));
  if (evidence.schemaVersion !== 1 || evidence.ok !== true || evidence.fixture !== 'isolated-loopback-tls') {
    throw new Error('真实 TLS 长稳报告结构或通过状态无效');
  }
  process.stdout.write(`${JSON.stringify({
    ok: true,
    report: path.relative(workspaceRoot, options.reportPath),
    durationSeconds: evidence.durationSeconds,
    sampleCount: evidence.sampleCount,
    peakResidentBytes: evidence.memory?.peakResidentBytes,
    growthBytes: evidence.memory?.growthBytes,
  }, null, 2)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  run().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.stack ?? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
