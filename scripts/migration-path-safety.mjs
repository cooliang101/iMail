import { existsSync } from 'node:fs';
import path from 'node:path';

function isSameOrInside(parent, candidate) {
  const relative = path.relative(parent, candidate);
  return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative));
}

export function directoriesOverlap(left, right) {
  const resolvedLeft = path.resolve(left);
  const resolvedRight = path.resolve(right);
  return isSameOrInside(resolvedLeft, resolvedRight) || isSameOrInside(resolvedRight, resolvedLeft);
}

export function assertNonOverlappingDirectories(source, target, label = '目标目录') {
  const resolvedSource = path.resolve(source);
  const resolvedTarget = path.resolve(target);
  if (directoriesOverlap(resolvedSource, resolvedTarget)) {
    throw new Error(`${label}不能与活动数据目录相同或互相包含：${resolvedTarget}`);
  }
  return resolvedTarget;
}

export function normalizeMigrationRunId(value) {
  const runId = String(value ?? '').trim();
  if (!/^[a-z0-9][a-z0-9._-]{0,79}$/i.test(runId) || runId === '.' || runId === '..') {
    throw new Error('迁移基线标识只能包含字母、数字、点、下划线和连字符，长度为 1–80');
  }
  return runId;
}

export function migrationOutputRoot(workspaceRoot, runId) {
  return path.join(path.resolve(workspaceRoot), 'output', 'rust-migration-tests', normalizeMigrationRunId(runId));
}

export function assertSafeMigrationOutput({ workspaceRoot, dataDir, outputRoot }) {
  const workspace = path.resolve(workspaceRoot);
  const allowedRoot = path.join(workspace, 'output', 'rust-migration-tests');
  const output = path.resolve(outputRoot);
  if (output === allowedRoot || !isSameOrInside(allowedRoot, output)) {
    throw new Error(`迁移基线只能写入 ${allowedRoot} 的独立子目录`);
  }
  assertNonOverlappingDirectories(dataDir, output, '迁移基线目录');
  if (existsSync(output)) throw new Error(`迁移基线目录已存在，拒绝覆盖：${output}`);
  return output;
}
