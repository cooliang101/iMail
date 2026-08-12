import { execFileSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { DatabaseSync } from 'node:sqlite';
import { afterEach, describe, expect, it } from 'vitest';
import { assertNonOverlappingDirectories, assertSafeMigrationOutput, directoriesOverlap, migrationOutputRoot, normalizeMigrationRunId } from './migration-path-safety.mjs';

const createdRoots = [];
afterEach(() => {
  for (const root of createdRoots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function fileHash(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

describe('Rust migration data protection', () => {
  it('rejects overlapping, broad, existing, and malformed migration targets', () => {
    const workspace = path.join(tmpdir(), `imail-migration-safety-${randomUUID()}`);
    createdRoots.push(workspace);
    const data = path.join(workspace, '.data');
    mkdirSync(data, { recursive: true });
    expect(directoriesOverlap(data, path.join(data, 'snapshot'))).toBe(true);
    expect(directoriesOverlap(data, path.join(workspace, 'backups', 'snapshot'))).toBe(false);
    expect(() => assertNonOverlappingDirectories(data, data)).toThrow(/活动数据目录/);
    expect(() => assertNonOverlappingDirectories(data, path.join(data, 'snapshot'))).toThrow(/活动数据目录/);
    expect(() => assertNonOverlappingDirectories(data, workspace)).toThrow(/活动数据目录/);
    expect(normalizeMigrationRunId('baseline-2026.08.10')).toBe('baseline-2026.08.10');
    expect(() => normalizeMigrationRunId('../.data')).toThrow(/迁移基线标识/);
    expect(() => assertSafeMigrationOutput({ workspaceRoot: workspace, dataDir: data, outputRoot: data })).toThrow(/独立子目录/);
    expect(() => assertSafeMigrationOutput({ workspaceRoot: workspace, dataDir: data, outputRoot: workspace })).toThrow(/独立子目录/);
    const allowed = migrationOutputRoot(workspace, 'safe-run');
    expect(assertSafeMigrationOutput({ workspaceRoot: workspace, dataDir: data, outputRoot: allowed })).toBe(allowed);
    mkdirSync(allowed, { recursive: true });
    expect(() => assertSafeMigrationOutput({ workspaceRoot: workspace, dataDir: data, outputRoot: allowed })).toThrow(/拒绝覆盖/);
  });

  it('creates a non-overwriting baseline snapshot without changing the source data', () => {
    const root = path.join(tmpdir(), `imail-migration-baseline-${randomUUID()}`);
    createdRoots.push(root);
    const data = path.join(root, 'active');
    mkdirSync(path.join(data, 'sender-logos'), { recursive: true });
    const databasePath = path.join(data, 'imail.sqlite');
    const database = new DatabaseSync(databasePath);
    database.exec("CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE accounts (id TEXT PRIMARY KEY); INSERT INTO metadata VALUES ('schema_version', '6'); INSERT INTO accounts VALUES ('account-1');");
    database.close();
    writeFileSync(path.join(data, 'master.key'), '42'.repeat(32));
    writeFileSync(path.join(data, 'instance-id'), '11111111-1111-4111-8111-111111111111\n');
    writeFileSync(path.join(data, 'sender-logos', 'logo.png'), 'logo-data');
    const before = fileHash(databasePath);
    const runId = `test-${randomUUID()}`;
    const outputRoot = migrationOutputRoot(process.cwd(), runId);
    createdRoots.push(outputRoot);
    const output = execFileSync(process.execPath, ['scripts/migration-baseline.mjs', runId], {
      cwd: process.cwd(), env: { ...process.env, IMAIL_DATA_DIR: data }, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
    });
    const result = JSON.parse(output);
    const report = JSON.parse(readFileSync(path.join(outputRoot, 'baseline-report.json'), 'utf8'));
    expect(result).toMatchObject({ ok: true, schemaVersion: 6, tableCounts: { accounts: 1, metadata: 1 } });
    expect(report).toMatchObject({
      formatVersion: 1,
      runId,
      snapshot: { masterKeyIncluded: true, instanceIdIncluded: true, senderLogoFileCount: 1 },
      database: { schemaVersion: 6, quickCheck: true, foreignKeysVerified: true, tableCounts: { accounts: 1, metadata: 1 } },
    });
    expect(fileHash(databasePath)).toBe(before);
    expect(existsSync(path.join(outputRoot, 'snapshot', 'backup-manifest.json'))).toBe(true);
    expect(() => execFileSync(process.execPath, ['scripts/migration-baseline.mjs', runId], {
      cwd: process.cwd(), env: { ...process.env, IMAIL_DATA_DIR: data }, stdio: 'pipe',
    })).toThrow();
    expect(fileHash(databasePath)).toBe(before);
  });
});
