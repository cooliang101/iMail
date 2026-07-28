import { createCipheriv, createDecipheriv, randomBytes } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import type { AccountSecret } from './types.js';

const dataDir = path.resolve('.data');
const keyFile = path.join(dataDir, 'master.key');

async function loadKey(): Promise<Buffer> {
  const configured = process.env.APP_MASTER_KEY?.trim();
  if (configured) {
    const key = Buffer.from(configured, 'hex');
    if (key.length !== 32) throw new Error('APP_MASTER_KEY 必须是 32 字节的十六进制值');
    return key;
  }
  await mkdir(dataDir, { recursive: true });
  try {
    const key = Buffer.from((await readFile(keyFile, 'utf8')).trim(), 'hex');
    if (key.length === 32) return key;
  } catch { /* 首次运行 */ }
  const key = randomBytes(32);
  await writeFile(keyFile, key.toString('hex'), { mode: 0o600 });
  return key;
}

export async function encryptSecret(secret: AccountSecret): Promise<string> {
  const key = await loadKey();
  const iv = randomBytes(12);
  const cipher = createCipheriv('aes-256-gcm', key, iv);
  const encrypted = Buffer.concat([cipher.update(JSON.stringify(secret), 'utf8'), cipher.final()]);
  return [iv, cipher.getAuthTag(), encrypted].map((value) => value.toString('base64url')).join('.');
}

export async function decryptSecret(payload: string): Promise<AccountSecret> {
  const key = await loadKey();
  const [iv, tag, encrypted] = payload.split('.').map((value) => Buffer.from(value, 'base64url'));
  if (!iv || !tag || !encrypted) throw new Error('账户凭据已损坏');
  const decipher = createDecipheriv('aes-256-gcm', key, iv);
  decipher.setAuthTag(tag);
  return JSON.parse(Buffer.concat([decipher.update(encrypted), decipher.final()]).toString('utf8')) as AccountSecret;
}
