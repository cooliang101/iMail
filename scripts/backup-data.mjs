import { cpSync, existsSync, mkdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { DatabaseSync, backup } from 'node:sqlite';
import packageMetadata from '../package.json' with { type: 'json' };
import { backupFileHashes } from './backup-integrity.mjs';

const dataDir = path.resolve(process.env.IMAIL_DATA_DIR ?? '.data');
const sourceDatabase = path.join(dataDir, 'imail.sqlite');
if (!existsSync(sourceDatabase)) throw new Error(`找不到 iMail 数据库：${sourceDatabase}`);

const safeTimestamp = new Date().toISOString().replaceAll(':', '-');
const backupRoot = path.resolve(process.argv[2] || path.join(process.env.IMAIL_BACKUP_DIR ?? 'backups', safeTimestamp));
if (existsSync(backupRoot)) throw new Error(`备份目标已存在：${backupRoot}`);
const stagingRoot = `${backupRoot}.partial-${process.pid}`;
if (existsSync(stagingRoot)) throw new Error(`备份暂存目标已存在：${stagingRoot}`);
mkdirSync(path.dirname(backupRoot), { recursive: true });
mkdirSync(stagingRoot);

try {
  const database = new DatabaseSync(sourceDatabase, { readOnly: true });
  let schemaVersion;
  try {
    const schema = database.prepare("SELECT value FROM metadata WHERE key = 'schema_version'").get();
    schemaVersion = Number(schema?.value);
    if (!Number.isSafeInteger(schemaVersion) || schemaVersion < 1) throw new Error('iMail 数据库缺少有效的 schema 版本');
    await backup(database, path.join(stagingRoot, 'imail.sqlite'));
  }
  finally { database.close(); }

  for (const item of ['master.key', 'instance-id', 'sender-logos']) {
    const source = path.join(dataDir, item);
    if (existsSync(source)) cpSync(source, path.join(stagingRoot, item), { recursive: true, errorOnExist: true });
  }
  writeFileSync(path.join(stagingRoot, 'backup-manifest.json'), `${JSON.stringify({
    formatVersion: 2,
    service: 'imail',
    serviceVersion: process.env.IMAIL_VERSION?.trim() || process.env.npm_package_version?.trim() || packageMetadata.version,
    schemaVersion,
    createdAt: new Date().toISOString(),
    files: backupFileHashes(stagingRoot),
  }, null, 2)}\n`, { encoding: 'utf8', flag: 'wx', mode: 0o600 });
  renameSync(stagingRoot, backupRoot);
} catch (error) {
  rmSync(stagingRoot, { recursive: true, force: true });
  throw error;
}

console.log(`iMail 备份已写入：${backupRoot}`);
