import { randomUUID, timingSafeEqual } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import type { ServiceInfo } from '../src/types.js';

export const SERVICE_PROTOCOL_VERSION = 1;
export const SERVICE_VERSION = process.env.IMAIL_VERSION?.trim() || process.env.npm_package_version?.trim() || '0.1.0';

let cachedInstanceId: string | undefined;

function instanceFile() {
  return path.join(path.resolve(process.env.IMAIL_DATA_DIR ?? '.data'), 'instance-id');
}

function validInstanceId(value: string) {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value);
}

export function serviceInstanceId() {
  if (cachedInstanceId) return cachedInstanceId;
  const file = instanceFile();
  try {
    const existing = readFileSync(file, 'utf8').trim();
    if (validInstanceId(existing)) return cachedInstanceId = existing;
  } catch { /* Create the identity below. */ }
  mkdirSync(path.dirname(file), { recursive: true });
  const created = randomUUID();
  try { writeFileSync(file, `${created}\n`, { encoding: 'utf8', flag: 'wx', mode: 0o600 }); }
  catch {
    const existing = readFileSync(file, 'utf8').trim();
    if (!validInstanceId(existing)) throw new Error('iMail 服务实例身份文件无效');
    return cachedInstanceId = existing;
  }
  return cachedInstanceId = created;
}

export function serviceInfo(webClient = false): ServiceInfo {
  return {
    service: 'imail',
    instanceId: serviceInstanceId(),
    version: SERVICE_VERSION,
    protocolVersion: SERVICE_PROTOCOL_VERSION,
    capabilities: { gateway: true, mcp: true, syncWorker: true, webClient },
  };
}

export function resetServiceInfoForTests() {
  cachedInstanceId = undefined;
}

export function daemonShutdownAuthorized(remoteAddress: string | undefined, providedToken: string | undefined, expectedToken: string | undefined) {
  const loopback = remoteAddress === '127.0.0.1' || remoteAddress === '::1' || remoteAddress === '::ffff:127.0.0.1';
  if (!loopback || !providedToken || !expectedToken) return false;
  const provided = Buffer.from(providedToken);
  const expected = Buffer.from(expectedToken.trim());
  return provided.length === expected.length && timingSafeEqual(provided, expected);
}
