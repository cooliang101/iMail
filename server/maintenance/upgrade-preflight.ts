import { execFileSync } from 'node:child_process';
import { existsSync, rmSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { DatabaseSync } from 'node:sqlite';
import { fileURLToPath } from 'node:url';
import packageMetadata from '../../package.json' with { type: 'json' };
import { ensureSchema } from '../storage/schema.js';

const backupRoot = path.resolve(process.argv[2] || '');
const preflightRoot = path.resolve(process.argv[3] || '');
if (!process.argv[2] || !process.argv[3]) {
  throw new Error('用法：npm run upgrade:preflight -- <新备份目录> <新迁移预检目录>');
}
if (existsSync(backupRoot)) throw new Error(`备份目标已存在：${backupRoot}`);
if (existsSync(preflightRoot)) throw new Error(`迁移预检目标已存在：${preflightRoot}`);

const moduleDir = path.dirname(fileURLToPath(import.meta.url));
const bundledBackupTool = path.join(moduleDir, 'imail-backup.mjs');
const bundledRestoreTool = path.join(moduleDir, 'imail-restore.mjs');
const repositoryRoot = path.resolve(moduleDir, '..', '..');
const backupTool = existsSync(bundledBackupTool) ? bundledBackupTool : path.join(repositoryRoot, 'scripts', 'backup-data.mjs');
const restoreTool = existsSync(bundledRestoreTool) ? bundledRestoreTool : path.join(repositoryRoot, 'scripts', 'prepare-restore.mjs');

execFileSync(process.execPath, [backupTool, backupRoot], { stdio: ['ignore', 'ignore', 'inherit'], env: process.env });

let restored = false;
try {
  const restoreOutput = execFileSync(process.execPath, [restoreTool, backupRoot, preflightRoot], {
    encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], env: process.env,
  });
  restored = true;
  const restore = JSON.parse(restoreOutput) as { schemaVersion: number; integrityManifestVerified: boolean };
  const databasePath = path.join(preflightRoot, 'imail.sqlite');
  const database = new DatabaseSync(databasePath, { timeout: 5_000 });
  let migratedSchemaVersion = 0;
  try {
    database.exec('PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;');
    ensureSchema(database);
    const checks = database.prepare('PRAGMA quick_check').all() as Array<{ quick_check: string }>;
    if (checks.length !== 1 || checks[0]?.quick_check !== 'ok') throw new Error('迁移后 SQLite quick_check 未通过');
    const foreignKeyErrors = database.prepare('PRAGMA foreign_key_check').all();
    if (foreignKeyErrors.length) throw new Error(`迁移后存在 ${foreignKeyErrors.length} 个外键错误`);
    migratedSchemaVersion = Number((database.prepare("SELECT value FROM metadata WHERE key = 'schema_version'").get() as { value?: string } | undefined)?.value);
    if (migratedSchemaVersion !== packageMetadata.imail.schemaVersion) {
      throw new Error(`迁移预检 schema v${migratedSchemaVersion} 与当前发布 v${packageMetadata.imail.schemaVersion} 不一致`);
    }
  } finally { database.close(); }

  console.log(JSON.stringify({
    ok: true,
    activeDataUntouched: true,
    backupRoot,
    preflightRoot,
    backupSchemaVersion: restore.schemaVersion,
    migratedSchemaVersion,
    integrityManifestVerified: restore.integrityManifestVerified,
    sqliteQuickCheck: true,
    foreignKeysVerified: true,
  }));
} catch (error) {
  if (restored && existsSync(preflightRoot)) rmSync(preflightRoot, { recursive: true, force: true });
  throw error;
}
