import { createHash } from 'node:crypto';
import { lookup } from 'node:dns/promises';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import net from 'node:net';
import path from 'node:path';
import type { CachedMessage } from './types.js';

type CachedLogo = { contentType: string; sourceUrl: string; fetchedAt: string };
type MissingLogo = { unavailableAt: string };
export type LogoResult = { content: Buffer; contentType: string; sourceUrl: string };

const pending = new Map<string, Promise<LogoResult | null>>();
const MAX_HTML_BYTES = 512 * 1024;
const MAX_IMAGE_BYTES = 1024 * 1024;
const NEGATIVE_CACHE_MS = 7 * 24 * 60 * 60 * 1000;

function cacheDirectory() {
  return path.join(path.resolve(process.env.IMAIL_DATA_DIR ?? '.data'), 'sender-logos');
}

function senderKey(message: CachedMessage) {
  return message.from.address.trim().toLowerCase() || message.from.name.trim().toLowerCase();
}

function cachePaths(key: string) {
  const name = createHash('sha256').update(key).digest('hex');
  const directory = cacheDirectory();
  return { directory, image: path.join(directory, `${name}.bin`), meta: path.join(directory, `${name}.json`) };
}

function isPublicIp(address: string) {
  const normalized = address.toLowerCase().replace(/^::ffff:/, '');
  if (net.isIPv4(normalized)) {
    const [a, b] = normalized.split('.').map(Number);
    return !(a === 0 || a === 10 || a === 127 || (a === 169 && b === 254) || (a === 172 && b >= 16 && b <= 31)
      || (a === 192 && b === 168) || (a === 100 && b >= 64 && b <= 127) || a >= 224);
  }
  if (net.isIPv6(normalized)) {
    return !(normalized === '::' || normalized === '::1' || normalized.startsWith('fc') || normalized.startsWith('fd')
      || /^fe[89ab]/.test(normalized) || normalized.startsWith('ff'));
  }
  return false;
}

async function assertPublicUrl(url: URL) {
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error('不安全的网址');
  if (url.port && !['80', '443'].includes(url.port)) throw new Error('不允许的端口');
  const addresses = net.isIP(url.hostname) ? [url.hostname] : (await lookup(url.hostname, { all: true })).map((item) => item.address);
  if (addresses.length === 0 || addresses.some((address) => !isPublicIp(address))) throw new Error('不允许访问内网地址');
}

async function safeFetch(initialUrl: URL, accepts: string, signal: AbortSignal, redirects = 3): Promise<Response> {
  let current = initialUrl;
  for (let index = 0; index <= redirects; index += 1) {
    await assertPublicUrl(current);
    const response = await fetch(current, {
      redirect: 'manual', signal,
      headers: { Accept: accepts, 'User-Agent': 'iMail Logo Fetcher/1.0' },
    });
    if (response.status >= 300 && response.status < 400) {
      const location = response.headers.get('location');
      if (!location || index === redirects) throw new Error('重定向过多');
      current = new URL(location, current);
      continue;
    }
    if (!response.ok) throw new Error(`远程请求失败：${response.status}`);
    return response;
  }
  throw new Error('重定向过多');
}

async function readLimited(response: Response, limit: number) {
  const declared = Number(response.headers.get('content-length') ?? 0);
  if (declared > limit) throw new Error('远程内容过大');
  if (!response.body) return Buffer.alloc(0);
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) { await reader.cancel(); throw new Error('远程内容过大'); }
    chunks.push(value);
  }
  return Buffer.concat(chunks);
}

function imageType(content: Buffer) {
  if (content.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))) return 'image/png';
  if (content[0] === 0xff && content[1] === 0xd8 && content[2] === 0xff) return 'image/jpeg';
  if (['GIF87a', 'GIF89a'].includes(content.subarray(0, 6).toString('ascii'))) return 'image/gif';
  if (content.subarray(0, 4).toString('ascii') === 'RIFF' && content.subarray(8, 12).toString('ascii') === 'WEBP') return 'image/webp';
  if (content[0] === 0 && content[1] === 0 && content[2] === 1 && content[3] === 0) return 'image/x-icon';
  return null;
}

function siteCandidates(message: CachedMessage) {
  const combined = `${message.html ?? ''}\n${message.text ?? ''}`;
  const urls: URL[] = [];
  const pattern = /(?:href\s*=\s*["']([^"']+)["']|https?:\/\/[^\s<>"']+)/gi;
  for (const match of combined.matchAll(pattern)) {
    const value = match[1] ?? match[0];
    try {
      const parsed = new URL(value.replace(/&amp;/g, '&'));
      if (!['http:', 'https:'].includes(parsed.protocol)) continue;
      if (/unsubscribe|optout|tracking|\/track|\/click/i.test(`${parsed.hostname}${parsed.pathname}`)) continue;
      urls.push(new URL(parsed.origin));
    } catch { /* Ignore malformed links in untrusted email content. */ }
  }
  const domain = message.from.address.split('@').at(-1)?.trim().toLowerCase();
  if (domain && /^[a-z0-9.-]+$/i.test(domain)) urls.push(new URL(`https://${domain}`));
  return [...new Map(urls.map((url) => [url.origin, url])).values()].slice(0, 6);
}

function discoverIcons(html: string, pageUrl: URL) {
  const icons: URL[] = [];
  for (const tag of html.match(/<link\b[^>]*>/gi) ?? []) {
    const rel = tag.match(/\brel\s*=\s*["']([^"']+)["']/i)?.[1] ?? '';
    const href = tag.match(/\bhref\s*=\s*["']([^"']+)["']/i)?.[1];
    if (!href || !/(?:^|\s)(?:apple-touch-icon|icon)(?:\s|$)/i.test(rel)) continue;
    try { icons.push(new URL(href.replace(/&amp;/g, '&'), pageUrl)); } catch { /* Ignore malformed icon URLs. */ }
  }
  icons.push(new URL('/favicon.ico', pageUrl));
  return [...new Map(icons.map((url) => [url.href, url])).values()].slice(0, 5);
}

async function fetchLogo(message: CachedMessage): Promise<LogoResult | null> {
  const signal = AbortSignal.timeout(10_000);
  for (const site of siteCandidates(message)) {
    try {
      const page = await safeFetch(site, 'text/html,application/xhtml+xml', signal);
      const contentType = page.headers.get('content-type') ?? '';
      if (!contentType.includes('text/html') && !contentType.includes('application/xhtml+xml')) continue;
      const html = (await readLimited(page, MAX_HTML_BYTES)).toString('utf8');
      for (const iconUrl of discoverIcons(html, new URL(page.url || site.href))) {
        try {
          const icon = await safeFetch(iconUrl, 'image/png,image/jpeg,image/webp,image/gif,image/x-icon', signal);
          const content = await readLimited(icon, MAX_IMAGE_BYTES);
          const detected = imageType(content);
          if (detected) return { content, contentType: detected, sourceUrl: icon.url || iconUrl.href };
        } catch { /* Try the next declared icon. */ }
      }
    } catch { /* Try the next website candidate. */ }
  }
  return null;
}

async function readCache(key: string): Promise<LogoResult | null | undefined> {
  const files = cachePaths(key);
  try {
    const meta = JSON.parse(await readFile(files.meta, 'utf8')) as CachedLogo | MissingLogo;
    if ('unavailableAt' in meta) return Date.now() - new Date(meta.unavailableAt).getTime() < NEGATIVE_CACHE_MS ? null : undefined;
    return { content: await readFile(files.image), contentType: meta.contentType, sourceUrl: meta.sourceUrl };
  } catch { return undefined; }
}

async function persist(key: string, result: LogoResult | null) {
  const files = cachePaths(key);
  await mkdir(files.directory, { recursive: true });
  if (!result) { await writeFile(files.meta, JSON.stringify({ unavailableAt: new Date().toISOString() } satisfies MissingLogo)); return; }
  const temporary = `${files.image}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(temporary, result.content);
  await rename(temporary, files.image);
  await writeFile(files.meta, JSON.stringify({ contentType: result.contentType, sourceUrl: result.sourceUrl, fetchedAt: new Date().toISOString() } satisfies CachedLogo));
}

export async function senderLogo(message: CachedMessage) {
  const key = senderKey(message);
  if (!key) return null;
  const cached = await readCache(key);
  if (cached !== undefined) return cached;
  const active = pending.get(key);
  if (active) return active;
  const request = fetchLogo(message).then(async (result) => { await persist(key, result); return result; }).finally(() => pending.delete(key));
  pending.set(key, request);
  return request;
}

export const senderLogoInternals = { isPublicIp, siteCandidates, discoverIcons, imageType };
