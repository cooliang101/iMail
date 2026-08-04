import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { afterEach, describe, expect, it } from 'vitest';
import { ensureSchema } from './storage/schema.js';

const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

describe('backup and restore preparation', () => {
  it('round-trips a verified snapshot without overwriting the active data directory', () => {
    const root = mkdtempSync(path.join(tmpdir(), 'imail-backup-restore-'));
    roots.push(root);
    const data = path.join(root, 'active');
    const backup = path.join(root, 'backup');
    const restored = path.join(root, 'restored');
    mkdirSync(path.join(data, 'sender-logos'), { recursive: true });
    const database = new DatabaseSync(path.join(data, 'imail.sqlite'));
    database.exec("CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE accounts (id TEXT PRIMARY KEY); INSERT INTO metadata VALUES ('schema_version', '4'); INSERT INTO accounts VALUES ('before-backup');");
    database.close();
    writeFileSync(path.join(data, 'master.key'), '42'.repeat(32));
    const instanceId = '11111111-1111-4111-8111-111111111111';
    writeFileSync(path.join(data, 'instance-id'), `${instanceId}\n`);
    writeFileSync(path.join(data, 'sender-logos', 'logo.png'), 'logo-data');

    execFileSync(process.execPath, ['scripts/backup-data.mjs', backup], {
      cwd: process.cwd(), env: { ...process.env, IMAIL_DATA_DIR: data }, stdio: 'pipe',
    });
    const active = new DatabaseSync(path.join(data, 'imail.sqlite'));
    active.exec("INSERT INTO accounts VALUES ('after-backup')");
    active.close();

    const output = execFileSync(process.execPath, ['scripts/prepare-restore.mjs', backup, restored], {
      cwd: process.cwd(), encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
    });
    expect(JSON.parse(output)).toMatchObject({ ok: true, databaseVerified: true, integrityManifestVerified: true, backupServiceVersion: '0.0.1', schemaVersion: 4, supportedSchemaVersion: 4, masterKeyIncluded: true, instanceIdIncluded: true, senderLogosIncluded: true });
    expect(JSON.parse(readFileSync(path.join(backup, 'backup-manifest.json'), 'utf8'))).toMatchObject({ formatVersion: 2, service: 'imail', serviceVersion: '0.0.1', schemaVersion: 4 });
    const snapshot = new DatabaseSync(path.join(restored, 'imail.sqlite'), { readOnly: true });
    expect(snapshot.prepare('SELECT id FROM accounts ORDER BY id').all()).toEqual([{ id: 'before-backup' }]);
    snapshot.close();
    expect(readFileSync(path.join(restored, 'master.key'), 'utf8')).toBe('42'.repeat(32));
    expect(readFileSync(path.join(restored, 'instance-id'), 'utf8').trim()).toBe(instanceId);
    expect(readFileSync(path.join(restored, 'sender-logos', 'logo.png'), 'utf8')).toBe('logo-data');
    expect(() => execFileSync(process.execPath, ['scripts/prepare-restore.mjs', backup, restored], { cwd: process.cwd(), stdio: 'pipe' })).toThrow();

    const tamperedBackup = path.join(root, 'tampered-backup');
    const tamperedRestore = path.join(root, 'tampered-restore');
    cpSync(backup, tamperedBackup, { recursive: true });
    writeFileSync(path.join(tamperedBackup, 'sender-logos', 'logo.png'), 'replaced-logo');
    expect(() => execFileSync(process.execPath, ['scripts/prepare-restore.mjs', tamperedBackup, tamperedRestore], { cwd: process.cwd(), stdio: 'pipe' })).toThrow();
    expect(existsSync(tamperedRestore)).toBe(false);

    const mismatchedBackup = path.join(root, 'mismatched-backup');
    const mismatchedRestore = path.join(root, 'mismatched-restore');
    cpSync(backup, mismatchedBackup, { recursive: true });
    const manifestPath = path.join(mismatchedBackup, 'backup-manifest.json');
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
    writeFileSync(manifestPath, JSON.stringify({ ...manifest, schemaVersion: 3 }));
    expect(() => execFileSync(process.execPath, ['scripts/prepare-restore.mjs', mismatchedBackup, mismatchedRestore], { cwd: process.cwd(), stdio: 'pipe' })).toThrow();
    expect(existsSync(mismatchedRestore)).toBe(false);

    const legacyBackup = path.join(root, 'legacy-v1-backup');
    const legacyRestore = path.join(root, 'legacy-v1-restore');
    cpSync(backup, legacyBackup, { recursive: true });
    const legacyManifestPath = path.join(legacyBackup, 'backup-manifest.json');
    const legacyManifest = JSON.parse(readFileSync(legacyManifestPath, 'utf8'));
    delete legacyManifest.service;
    delete legacyManifest.serviceVersion;
    delete legacyManifest.schemaVersion;
    writeFileSync(legacyManifestPath, JSON.stringify({ ...legacyManifest, formatVersion: 1 }));
    const legacyOutput = execFileSync(process.execPath, ['scripts/prepare-restore.mjs', legacyBackup, legacyRestore], {
      cwd: process.cwd(), encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
    });
    expect(JSON.parse(legacyOutput)).toMatchObject({ ok: true, schemaVersion: 4, integrityManifestVerified: true, instanceIdIncluded: true });

    const futureBackup = path.join(root, 'future-schema-backup');
    const futureRestore = path.join(root, 'future-schema-restore');
    cpSync(backup, futureBackup, { recursive: true });
    const futureDatabase = new DatabaseSync(path.join(futureBackup, 'imail.sqlite'));
    futureDatabase.prepare("UPDATE metadata SET value = '999' WHERE key = 'schema_version'").run();
    futureDatabase.close();
    const futureManifestPath = path.join(futureBackup, 'backup-manifest.json');
    const futureManifest = JSON.parse(readFileSync(futureManifestPath, 'utf8'));
    futureManifest.schemaVersion = 999;
    futureManifest.files['imail.sqlite'] = createHash('sha256').update(readFileSync(path.join(futureBackup, 'imail.sqlite'))).digest('hex');
    writeFileSync(futureManifestPath, JSON.stringify(futureManifest));
    expect(() => execFileSync(process.execPath, ['scripts/prepare-restore.mjs', futureBackup, futureRestore], { cwd: process.cwd(), stdio: 'pipe' })).toThrow();
    expect(existsSync(futureRestore)).toBe(false);
  });

  it('preflights current migrations on a restored copy and preserves a rollback backup on failure', () => {
    const root = mkdtempSync(path.join(tmpdir(), 'imail-upgrade-preflight-'));
    roots.push(root);
    const data = path.join(root, 'active');
    const backup = path.join(root, 'rollback-backup');
    const preflight = path.join(root, 'migrated-copy');
    mkdirSync(data, { recursive: true });
    const database = new DatabaseSync(path.join(data, 'imail.sqlite'));
    ensureSchema(database);
    database.prepare("UPDATE metadata SET value = '3' WHERE key = 'schema_version'").run();
    database.close();
    writeFileSync(path.join(data, 'master.key'), '43'.repeat(32));

    const output = execFileSync(process.execPath, ['--import', 'tsx', 'server/maintenance/upgrade-preflight.ts', backup, preflight], {
      cwd: process.cwd(), env: { ...process.env, IMAIL_DATA_DIR: data }, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
    });
    expect(JSON.parse(output)).toMatchObject({
      ok: true, activeDataUntouched: true, backupSchemaVersion: 3, migratedSchemaVersion: 4,
      integrityManifestVerified: true, sqliteQuickCheck: true, foreignKeysVerified: true,
    });
    const active = new DatabaseSync(path.join(data, 'imail.sqlite'), { readOnly: true });
    expect(active.prepare("SELECT value FROM metadata WHERE key = 'schema_version'").get()).toEqual({ value: '3' });
    active.close();
    const migrated = new DatabaseSync(path.join(preflight, 'imail.sqlite'), { readOnly: true });
    expect(migrated.prepare("SELECT value FROM metadata WHERE key = 'schema_version'").get()).toEqual({ value: '4' });
    migrated.close();

    const futureData = path.join(root, 'future-active');
    const futureBackup = path.join(root, 'future-rollback-backup');
    const failedPreflight = path.join(root, 'failed-migrated-copy');
    mkdirSync(futureData);
    const future = new DatabaseSync(path.join(futureData, 'imail.sqlite'));
    ensureSchema(future);
    future.prepare("UPDATE metadata SET value = '999' WHERE key = 'schema_version'").run();
    future.close();
    expect(() => execFileSync(process.execPath, ['--import', 'tsx', 'server/maintenance/upgrade-preflight.ts', futureBackup, failedPreflight], {
      cwd: process.cwd(), env: { ...process.env, IMAIL_DATA_DIR: futureData }, stdio: 'pipe',
    })).toThrow();
    expect(existsSync(futureBackup)).toBe(true);
    expect(existsSync(failedPreflight)).toBe(false);
  });
});
