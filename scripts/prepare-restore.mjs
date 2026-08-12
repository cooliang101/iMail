import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { DatabaseSync } from 'node:sqlite';
import packageMetadata from '../frontend/package.json' with { type: 'json' };
import { verifyBackupManifest } from './backup-integrity.mjs';

const backupRoot = path.resolve(process.argv[2] || '');
const restoreRoot = path.resolve(process.argv[3] || '');
if (!process.argv[2] || !process.argv[3]) {
  throw new Error('用法：npm --prefix frontend run restore:prepare -- <备份目录> <新的恢复目录>');
}
if (!existsSync(backupRoot)) throw new Error(`备份目录不存在：${backupRoot}`);
if (existsSync(restoreRoot)) throw new Error(`恢复目标已存在，拒绝覆盖：${restoreRoot}`);
const relativeTarget = path.relative(backupRoot, restoreRoot);
const relativeBackup = path.relative(restoreRoot, backupRoot);
if ((!relativeTarget.startsWith('..') && !path.isAbsolute(relativeTarget))
  || (!relativeBackup.startsWith('..') && !path.isAbsolute(relativeBackup))) {
  throw new Error('备份目录与恢复目录不能互相包含');
}

const sourceDatabase = path.join(backupRoot, 'imail.sqlite');
if (!existsSync(sourceDatabase)) throw new Error(`备份缺少数据库：${sourceDatabase}`);
const manifestPath = path.join(backupRoot, 'backup-manifest.json');
let manifest;
if (existsSync(manifestPath)) {
  try { manifest = JSON.parse(readFileSync(manifestPath, 'utf8')); }
  catch { throw new Error('备份完整性清单不是有效 JSON'); }
  verifyBackupManifest(backupRoot, manifest);
}
const sourceKey = path.join(backupRoot, 'master.key');
if (existsSync(sourceKey) && !/^[0-9a-f]{64}$/i.test(readFileSync(sourceKey, 'utf8').trim())) {
  throw new Error('备份中的 master.key 不是有效的 32 字节十六进制密钥');
}
const sourceInstanceId = path.join(backupRoot, 'instance-id');
if (existsSync(sourceInstanceId) && !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(readFileSync(sourceInstanceId, 'utf8').trim())) {
  throw new Error('备份中的 instance-id 不是有效的 iMail 实例身份');
}

function verifyDatabase(databasePath) {
  const database = new DatabaseSync(databasePath, { readOnly: true });
  try {
    const checks = database.prepare('PRAGMA quick_check').all();
    if (checks.length !== 1 || checks[0].quick_check !== 'ok') throw new Error('SQLite quick_check 未通过');
    const tables = new Set(database.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('accounts', 'metadata')").all().map((row) => row.name));
    if (!tables.has('accounts') || !tables.has('metadata')) throw new Error('数据库不是有效的 iMail 数据库');
    const schemaVersion = Number(database.prepare("SELECT value FROM metadata WHERE key = 'schema_version'").get()?.value);
    if (!Number.isSafeInteger(schemaVersion) || schemaVersion < 1) throw new Error('iMail 数据库缺少有效的 schema 版本');
    return { schemaVersion };
  } finally {
    database.close();
  }
}

const databaseMetadata = verifyDatabase(sourceDatabase);
const supportedSchemaVersion = packageMetadata.imail.schemaVersion;
if (databaseMetadata.schemaVersion > supportedSchemaVersion) {
  throw new Error(`备份数据库 schema v${databaseMetadata.schemaVersion} 高于当前 iMail 支持的 v${supportedSchemaVersion}`);
}
if (manifest?.formatVersion === 2 && manifest.schemaVersion !== databaseMetadata.schemaVersion) {
  throw new Error(`备份清单 schema v${manifest.schemaVersion} 与数据库 schema v${databaseMetadata.schemaVersion} 不一致`);
}
let created = false;
try {
  mkdirSync(restoreRoot, { recursive: false });
  created = true;
  cpSync(sourceDatabase, path.join(restoreRoot, 'imail.sqlite'), { errorOnExist: true });
  if (existsSync(sourceKey)) {
    cpSync(sourceKey, path.join(restoreRoot, 'master.key'), { errorOnExist: true });
    if (process.platform !== 'win32') chmodSync(path.join(restoreRoot, 'master.key'), 0o600);
  }
  if (existsSync(sourceInstanceId)) cpSync(sourceInstanceId, path.join(restoreRoot, 'instance-id'), { errorOnExist: true });
  const sourceLogos = path.join(backupRoot, 'sender-logos');
  if (existsSync(sourceLogos)) cpSync(sourceLogos, path.join(restoreRoot, 'sender-logos'), { recursive: true, errorOnExist: true });
  verifyDatabase(path.join(restoreRoot, 'imail.sqlite'));
} catch (error) {
  if (created) rmSync(restoreRoot, { recursive: true, force: true });
  throw error;
}

console.log(JSON.stringify({
  ok: true,
  restoreRoot,
  databaseVerified: true,
  integrityManifestVerified: existsSync(manifestPath),
  backupServiceVersion: manifest?.serviceVersion,
  schemaVersion: databaseMetadata.schemaVersion,
  supportedSchemaVersion,
  masterKeyIncluded: existsSync(sourceKey),
  instanceIdIncluded: existsSync(sourceInstanceId),
  senderLogosIncluded: existsSync(path.join(backupRoot, 'sender-logos')),
}));
