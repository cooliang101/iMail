import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { DatabaseSync } from 'node:sqlite';
import packageMetadata from '../package.json' with { type: 'json' };
import { assertSafeMigrationOutput, migrationOutputRoot, normalizeMigrationRunId } from './migration-path-safety.mjs';

const workspaceRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function safeTimestamp() {
  return new Date().toISOString().replaceAll(':', '-').replace('T', '_').replace('Z', 'Z');
}

function sha256File(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

function countFiles(root) {
  if (!existsSync(root)) return 0;
  let count = 0;
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const absolute = path.join(root, entry.name);
    if (entry.isDirectory()) count += countFiles(absolute);
    else if (entry.isFile()) count += 1;
  }
  return count;
}

function databaseInventory(databasePath) {
  const database = new DatabaseSync(databasePath, { readOnly: true });
  try {
    database.exec('PRAGMA query_only = ON; PRAGMA foreign_keys = ON;');
    const quickCheck = database.prepare('PRAGMA quick_check').all();
    const quickCheckOk = quickCheck.length === 1 && quickCheck[0].quick_check === 'ok';
    if (!quickCheckOk) throw new Error('SQLite quick_check 未通过');
    const foreignKeyViolations = database.prepare('PRAGMA foreign_key_check').all().length;
    if (foreignKeyViolations !== 0) throw new Error(`SQLite foreign_key_check 发现 ${foreignKeyViolations} 项异常`);
    const schemaVersion = Number(database.prepare("SELECT value FROM metadata WHERE key = 'schema_version'").get()?.value);
    if (!Number.isSafeInteger(schemaVersion) || schemaVersion < 1) throw new Error('iMail 数据库缺少有效的 schema 版本');
    const tableNames = database.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
      .all().map((row) => String(row.name)).filter((name) => /^[a-z][a-z0-9_]*$/i.test(name));
    const tableCounts = Object.fromEntries(tableNames.map((name) => [
      name,
      Number(database.prepare(`SELECT count(*) AS count FROM "${name}"`).get().count),
    ]));
    return { schemaVersion, quickCheck: true, foreignKeysVerified: true, tableCounts };
  } finally {
    database.close();
  }
}

export function createMigrationBaseline({
  dataDir = path.resolve(process.env.IMAIL_DATA_DIR ?? '.data'),
  runId = safeTimestamp(),
  root = workspaceRoot,
} = {}) {
  const normalizedRunId = normalizeMigrationRunId(runId);
  const sourceData = path.resolve(dataDir);
  const sourceDatabase = path.join(sourceData, 'imail.sqlite');
  if (!existsSync(sourceDatabase) || !statSync(sourceDatabase).isFile()) {
    throw new Error(`找不到 iMail 数据库：${sourceDatabase}`);
  }
  const outputRoot = assertSafeMigrationOutput({
    workspaceRoot: root,
    dataDir: sourceData,
    outputRoot: migrationOutputRoot(root, normalizedRunId),
  });
  const snapshotRoot = path.join(outputRoot, 'snapshot');
  const sourceDatabaseFileSizeAtStart = statSync(sourceDatabase).size;
  const sourceDatabaseSha256AtStart = sha256File(sourceDatabase);
  mkdirSync(outputRoot, { recursive: true });
  execFileSync(process.execPath, [path.join(root, 'scripts', 'backup-data.mjs'), snapshotRoot], {
    cwd: root,
    env: { ...process.env, IMAIL_DATA_DIR: sourceData },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const manifest = JSON.parse(readFileSync(path.join(snapshotRoot, 'backup-manifest.json'), 'utf8'));
  const database = databaseInventory(path.join(snapshotRoot, 'imail.sqlite'));
  const sourceDatabaseSha256AtEnd = sha256File(sourceDatabase);
  const report = {
    formatVersion: 1,
    service: 'imail-rust-migration-baseline',
    serviceVersion: process.env.IMAIL_VERSION?.trim() || process.env.npm_package_version?.trim() || packageMetadata.version,
    createdAt: new Date().toISOString(),
    runId: normalizedRunId,
    source: {
      dataDirectory: sourceData,
      databaseFileSizeAtStart: sourceDatabaseFileSizeAtStart,
      databaseSha256AtStart: sourceDatabaseSha256AtStart,
      databaseSha256AtEnd: sourceDatabaseSha256AtEnd,
      databaseChangedDuringSnapshot: sourceDatabaseSha256AtStart !== sourceDatabaseSha256AtEnd,
    },
    snapshot: {
      directory: snapshotRoot,
      manifestFormatVersion: manifest.formatVersion,
      databaseSha256: manifest.files?.['imail.sqlite'],
      masterKeyIncluded: typeof manifest.files?.['master.key'] === 'string',
      masterKeyFingerprint: manifest.files?.['master.key'],
      instanceIdIncluded: typeof manifest.files?.['instance-id'] === 'string',
      instanceIdFingerprint: manifest.files?.['instance-id'],
      senderLogoFileCount: countFiles(path.join(snapshotRoot, 'sender-logos')),
    },
    database,
  };
  const reportPath = path.join(outputRoot, 'baseline-report.json');
  writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, { encoding: 'utf8', flag: 'wx', mode: 0o600 });
  return { outputRoot, snapshotRoot, reportPath, report };
}

const entry = process.argv[1] ? path.resolve(process.argv[1]) : '';
if (entry === fileURLToPath(import.meta.url)) {
  const result = createMigrationBaseline({ runId: process.argv[2] || undefined });
  console.log(JSON.stringify({
    ok: true,
    outputRoot: result.outputRoot,
    snapshotRoot: result.snapshotRoot,
    reportPath: result.reportPath,
    schemaVersion: result.report.database.schemaVersion,
    tableCounts: result.report.database.tableCounts,
  }));
}
