import { createHash } from 'node:crypto';
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';

function walk(root, current, hashes) {
  for (const entry of readdirSync(current, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
    if (entry.name === 'backup-manifest.json' && current === root) continue;
    const absolute = path.join(current, entry.name);
    const relative = path.relative(root, absolute).split(path.sep).join('/');
    const metadata = lstatSync(absolute);
    if (metadata.isSymbolicLink()) throw new Error(`备份内容不能包含符号链接：${relative}`);
    if (metadata.isDirectory()) walk(root, absolute, hashes);
    else if (metadata.isFile()) hashes[relative] = createHash('sha256').update(readFileSync(absolute)).digest('hex');
    else throw new Error(`备份包含不支持的文件类型：${relative}`);
  }
}

export function backupFileHashes(root) {
  const hashes = {};
  walk(root, root, hashes);
  return hashes;
}

export function verifyBackupManifest(root, manifest) {
  if (!manifest || ![1, 2].includes(manifest.formatVersion) || typeof manifest.files !== 'object' || Array.isArray(manifest.files)) {
    throw new Error('备份完整性清单格式无效');
  }
  if (manifest.formatVersion === 2 && (manifest.service !== 'imail'
    || typeof manifest.serviceVersion !== 'string' || !manifest.serviceVersion
    || !Number.isSafeInteger(manifest.schemaVersion) || manifest.schemaVersion < 1)) {
    throw new Error('备份版本元数据无效');
  }
  const actual = backupFileHashes(root);
  const expectedKeys = Object.keys(manifest.files).sort();
  const actualKeys = Object.keys(actual).sort();
  if (JSON.stringify(expectedKeys) !== JSON.stringify(actualKeys)) throw new Error('备份文件集合与完整性清单不一致');
  for (const name of expectedKeys) {
    if (!/^[0-9a-f]{64}$/i.test(String(manifest.files[name])) || actual[name] !== manifest.files[name]) {
      throw new Error(`备份文件完整性校验失败：${name}`);
    }
  }
}
