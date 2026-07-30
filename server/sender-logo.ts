import { createHash } from 'node:crypto';
import { lookup } from 'node:dns/promises';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import http from 'node:http';
import https from 'node:https';
import net from 'node:net';
import path from 'node:path';
import { Readable } from 'node:stream';
import { contactDomain, contactLogoKey, contactLogoKeys, contactRootLogoKey } from './contact-model.js';
import { readStore, updateStore } from './store.js';
import type { CachedMessage } from './types.js';

type CachedLogo = { contentType: string; sourceUrl: string; fetchedAt: string };
type MissingLogo = { unavailableAt: string; version?: number; permanent?: boolean };
export type LogoResult = { content: Buffer; contentType: string; sourceUrl: string; fetchedAt: string; key: string };
type LogoSource = Pick<CachedMessage, 'from' | 'html' | 'text'>;

const PERMANENT_FAILURE = Symbol('permanent-logo-failure');
const pending = new Map<string, Promise<void>>();
const MAX_HTML_BYTES = 512 * 1024;
const MAX_IMAGE_BYTES = 1024 * 1024;
const FAILURE_CACHE_TTL_MS = 24 * 60 * 60_000;
const NEGATIVE_CACHE_VERSION = 3;

class PermanentLogoFailure extends Error {}

function cacheDirectory() {
  return path.join(path.resolve(process.env.IMAIL_DATA_DIR ?? '.data'), 'sender-logos');
}

function senderKey(message: LogoSource) {
  return contactLogoKey(message.from.address) ?? `sender:${message.from.address.trim().toLowerCase() || message.from.name.trim().toLowerCase()}`;
}

function rootSenderKey(message: LogoSource) {
  return contactRootLogoKey(message.from.address) ?? senderKey(message);
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

async function publicAddresses(url: URL) {
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error('不安全的网址');
  if (url.port && !['80', '443'].includes(url.port)) throw new Error('不允许的端口');
  const addresses = net.isIP(url.hostname)
    ? [{ address: url.hostname, family: net.isIPv6(url.hostname) ? 6 : 4 }]
    : await lookup(url.hostname, { all: true, verbatim: true });
  if (addresses.length === 0 || addresses.some((item) => !isPublicIp(item.address))) throw new Error('不允许访问内网地址');
  return addresses;
}

function pinnedRequestOptions(url: URL, selected: { address: string; family: number }, accepts: string, signal: AbortSignal) {
  return {
    protocol: url.protocol,
    hostname: selected.address,
    family: selected.family,
    port: url.port || (url.protocol === 'https:' ? 443 : 80),
    method: 'GET',
    path: `${url.pathname}${url.search}`,
    servername: url.protocol === 'https:' && !net.isIP(url.hostname) ? url.hostname : undefined,
    signal,
    headers: { Accept: accepts, 'User-Agent': 'iMail Logo Fetcher/1.0', Host: url.host },
  };
}

async function pinnedFetch(url: URL, accepts: string, signal: AbortSignal) {
  const addresses = await publicAddresses(url);
  const selected = addresses[0];
  const transport = url.protocol === 'https:' ? https : http;
  return new Promise<Response>((resolve, reject) => {
    const request = transport.request(pinnedRequestOptions(url, selected, accepts, signal), (incoming) => {
      const headers = new Headers();
      for (const [name, value] of Object.entries(incoming.headers)) {
        if (Array.isArray(value)) value.forEach((item) => headers.append(name, item));
        else if (value !== undefined) headers.set(name, String(value));
      }
      const status = incoming.statusCode ?? 500;
      const body = ['HEAD'].includes(request.method ?? '') || status === 204 || status === 304
        ? null
        : Readable.toWeb(incoming) as ReadableStream<Uint8Array>;
      const response = new Response(body, { status, statusText: incoming.statusMessage, headers });
      Object.defineProperty(response, 'url', { value: url.href });
      resolve(response);
    });
    request.once('error', reject);
    request.end();
  });
}

async function safeFetch(initialUrl: URL, accepts: string, signal: AbortSignal, redirects = 3): Promise<Response> {
  let current = initialUrl;
  for (let index = 0; index <= redirects; index += 1) {
    const response = await pinnedFetch(current, accepts, signal);
    if (response.status >= 300 && response.status < 400) {
      const location = response.headers.get('location');
      if (!location || index === redirects) throw new Error('重定向过多');
      current = new URL(location, current);
      continue;
    }
    if (isCloudflareForbidden(response)) throw new PermanentLogoFailure('Cloudflare 返回 403，停止重试');
    if (!response.ok) throw new Error(`远程请求失败：${response.status}`);
    return response;
  }
  throw new Error('重定向过多');
}

function isCloudflareForbidden(response: Response) {
  return response.status === 403 && (response.headers.has('cf-ray')
    || response.headers.has('cf-mitigated')
    || response.headers.get('server')?.toLocaleLowerCase().includes('cloudflare') === true);
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

function siteCandidates(message: LogoSource) {
  const html = message.html ?? '';
  const text = message.text ?? '';
  const urls: URL[] = [];
  const values = [
    ...Array.from(html.matchAll(/\bhref\s*=\s*["']([^"']+)["']/gi), (match) => match[1]),
    ...Array.from(text.matchAll(/https?:\/\/[^\s<>"']+/gi), (match) => match[0]),
  ];
  const domain = contactDomain(message.from.address);
  for (const value of values) {
    try {
      const parsed = new URL(value.replace(/&amp;/g, '&'));
      if (!['http:', 'https:'].includes(parsed.protocol)) continue;
      if (/unsubscribe|optout|tracking|\/track|\/click/i.test(`${parsed.hostname}${parsed.pathname}`)) continue;
      if (domain && contactDomain(`x@${parsed.hostname}`)?.registrable !== domain.registrable) continue;
      urls.push(new URL(parsed.origin));
    } catch { /* Ignore malformed links in untrusted email content. */ }
  }
  if (domain) {
    urls.push(new URL(`https://${domain.hostname}`));
    if (domain.hostname !== domain.registrable) urls.push(new URL(`https://${domain.registrable}`));
    urls.push(new URL(`https://www.${domain.registrable}`));
  }
  return [...new Map(urls.map((url) => [url.origin, url])).values()].slice(0, 8);
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

function isChallengePage(html: string) {
  return /(?:<title>\s*(?:just a moment|attention required[^<]*cloudflare)|cdn-cgi\/challenge-platform|cf-browser-verification|\bcf-chl-)/i.test(html);
}

async function fetchLogo(message: LogoSource): Promise<LogoResult | typeof PERMANENT_FAILURE | null> {
  const domainKey = senderKey(message);
  const attempted = new Set((await readStore()).logoFetchAttempts
    ?.filter((item) => item.status === 'success' || (isFreshFailure(item.attemptedAt) && !/^远程请求失败：403$/.test(item.detail)))
    .map((item) => item.target) ?? []);
  for (const site of siteCandidates(message)) {
    const target = site.origin.toLocaleLowerCase();
    if (attempted.has(target)) continue;
    let detail = '未找到有效的网站图标';
    let iconUrls = [new URL('/favicon.ico', site)];
    try {
      const page = await safeFetch(site, 'text/html,application/xhtml+xml', AbortSignal.timeout(10_000));
      const contentType = page.headers.get('content-type') ?? '';
      if (!contentType.includes('text/html') && !contentType.includes('application/xhtml+xml')) throw new Error('网站未返回 HTML');
      const html = (await readLimited(page, MAX_HTML_BYTES)).toString('utf8');
      if (isChallengePage(html)) throw new Error('网站返回了访问验证页');
      iconUrls = discoverIcons(html, new URL(page.url || site.href));
    } catch (error) {
      detail = errorMessage(error);
      if (error instanceof PermanentLogoFailure) {
        await recordAttempt(target, domainKey, 'failed', detail);
        return PERMANENT_FAILURE;
      }
    }
    for (const iconUrl of iconUrls) {
      try {
        const icon = await safeFetch(iconUrl, 'image/png,image/jpeg,image/webp,image/gif,image/x-icon', AbortSignal.timeout(10_000));
        const content = await readLimited(icon, MAX_IMAGE_BYTES);
        const detected = imageType(content);
        if (detected) {
          const result = { content, contentType: detected, sourceUrl: icon.url || iconUrl.href, fetchedAt: new Date().toISOString(), key: `domain:${site.hostname.toLocaleLowerCase()}` };
          await recordAttempt(target, domainKey, 'success', result.sourceUrl);
          return result;
        }
        detail = '图标内容不是受支持的图片格式';
      } catch (error) {
        detail = errorMessage(error);
        if (error instanceof PermanentLogoFailure) {
          await recordAttempt(target, domainKey, 'failed', detail);
          return PERMANENT_FAILURE;
        }
      }
    }
    await recordAttempt(target, domainKey, 'failed', detail);
    attempted.add(target);
  }
  return null;
}

function errorMessage(error: unknown) {
  return (error instanceof Error ? error.message : String(error)).replace(/[\r\n]+/g, ' ').slice(0, 300);
}

async function recordAttempt(target: string, domainKey: string, status: 'success' | 'failed', detail: string) {
  const attemptedAt = new Date().toISOString();
  await updateStore((data) => {
    data.logoFetchAttempts ??= [];
    const existing = data.logoFetchAttempts.find((item) => item.target === target);
    if (existing) Object.assign(existing, { domainKey, status, detail, attemptedAt });
    else data.logoFetchAttempts.push({ target, domainKey, status, detail, attemptedAt });
  });
  console.info(`[sender-logo] ${status} target=${target} domain=${domainKey} detail=${detail}`);
}

function isFreshFailure(attemptedAt: string, now = Date.now()) {
  const timestamp = Date.parse(attemptedAt);
  return Number.isFinite(timestamp) && now - timestamp < FAILURE_CACHE_TTL_MS;
}

async function readCache(key: string): Promise<LogoResult | null | undefined> {
  const files = cachePaths(key);
  try {
    const meta = JSON.parse(await readFile(files.meta, 'utf8')) as CachedLogo | MissingLogo;
    if ('unavailableAt' in meta) return meta.version === NEGATIVE_CACHE_VERSION && (meta.permanent || isFreshFailure(meta.unavailableAt)) ? null : undefined;
    return { content: await readFile(files.image), contentType: meta.contentType, sourceUrl: meta.sourceUrl, fetchedAt: meta.fetchedAt, key };
  } catch { return undefined; }
}

async function persist(key: string, result: LogoResult | null, permanent = false) {
  const files = cachePaths(key);
  await mkdir(files.directory, { recursive: true });
  if (!result) { await writeFile(files.meta, JSON.stringify({ unavailableAt: new Date().toISOString(), version: NEGATIVE_CACHE_VERSION, ...(permanent ? { permanent: true } : {}) } satisfies MissingLogo)); return; }
  const temporary = `${files.image}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(temporary, result.content);
  await rename(temporary, files.image);
  await writeFile(files.meta, JSON.stringify({ contentType: result.contentType, sourceUrl: result.sourceUrl, fetchedAt: result.fetchedAt } satisfies CachedLogo));
}

export async function senderLogo(message: LogoSource) {
  const keys = contactLogoKeys(message.from.address);
  const exactKey = keys?.exact ?? senderKey(message);
  const rootKey = keys?.root ?? exactKey;
  const exact = await readCache(exactKey);
  if (exact) return exact;
  if (rootKey !== exactKey) {
    const root = await readCache(rootKey);
    if (root) return root;
    if (exact === null && root === null) return null;
  } else if (exact === null) return null;

  const pendingKey = rootSenderKey(message);
  const active = pending.get(pendingKey);
  if (active) {
    await active;
    return (await readCache(exactKey)) || (rootKey !== exactKey ? (await readCache(rootKey)) || null : null);
  }
  const request = (async () => {
    const outcome = await fetchLogo(message);
    const permanent = outcome === PERMANENT_FAILURE;
    const result = permanent ? null : outcome;
    if (!result) {
      await persist(exactKey, null, permanent);
      if (rootKey !== exactKey && (permanent || await readCache(rootKey) === undefined)) await persist(rootKey, null, permanent);
      return;
    }
    await persist(result.key, result);
    if (result.key !== rootKey) {
      const root = await readCache(rootKey);
      if (!root) await persist(rootKey, { ...result, key: rootKey });
    }
  })().finally(() => pending.delete(pendingKey));
  pending.set(pendingKey, request);
  await request;
  return (await readCache(exactKey)) || (rootKey !== exactKey ? (await readCache(rootKey)) || null : null);
}

export const senderLogoInternals = { isPublicIp, pinnedRequestOptions, siteCandidates, discoverIcons, imageType, isChallengePage, isCloudflareForbidden, contactDomain, senderKey, rootSenderKey, isFreshFailure, FAILURE_CACHE_TTL_MS };
