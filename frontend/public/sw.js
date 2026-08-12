const CACHE_PREFIX = 'imail-web-';
const STATIC_CACHE = `${CACHE_PREFIX}static-v1`;
const PAGE_CACHE = `${CACHE_PREFIX}pages-v1`;
const MAX_STATIC_ENTRIES = 80;
const OFFLINE_URL = new URL('./', self.registration.scope).href;
const PRECACHE_URLS = [
  './',
  './manifest.webmanifest',
  './favicon.svg',
  './pwa-192.png',
  './pwa-512.png',
];

self.addEventListener('install', (event) => {
  event.waitUntil((async () => {
    const cache = await caches.open(PAGE_CACHE);
    await Promise.allSettled(PRECACHE_URLS.map((path) => cache.add(new URL(path, self.registration.scope))));
    await self.skipWaiting();
  })());
});

self.addEventListener('activate', (event) => {
  event.waitUntil((async () => {
    const active = new Set([STATIC_CACHE, PAGE_CACHE]);
    await Promise.all((await caches.keys())
      .filter((name) => name.startsWith(CACHE_PREFIX) && !active.has(name))
      .map((name) => caches.delete(name)));
    await self.clients.claim();
  })());
});

function isServiceRequest(url) {
  const scopePath = new URL(self.registration.scope).pathname.replace(/\/$/, '');
  const path = url.pathname.startsWith(scopePath) ? url.pathname.slice(scopePath.length) : url.pathname;
  return path === '/api' || path.startsWith('/api/')
    || path === '/gateway' || path.startsWith('/gateway/')
    || path === '/mcp' || path.startsWith('/mcp/');
}

function isStaticRequest(request, url) {
  return url.pathname.includes('/assets/')
    || ['script', 'style', 'font', 'image', 'manifest'].includes(request.destination);
}

async function cacheResponse(cacheName, request, response) {
  if (!response.ok || response.type === 'opaque') return;
  const cache = await caches.open(cacheName);
  await cache.put(request, response.clone());
  if (cacheName === STATIC_CACHE) {
    const keys = await cache.keys();
    await Promise.all(keys.slice(0, Math.max(0, keys.length - MAX_STATIC_ENTRIES)).map((key) => cache.delete(key)));
  }
}

async function networkFirstPage(request) {
  try {
    const response = await fetch(request);
    await cacheResponse(PAGE_CACHE, request, response);
    return response;
  } catch {
    return (await caches.match(request)) || (await caches.match(OFFLINE_URL)) || Response.error();
  }
}

async function cacheFirstStatic(request) {
  const cached = await caches.match(request);
  if (cached) return cached;
  const response = await fetch(request);
  await cacheResponse(STATIC_CACHE, request, response);
  return response;
}

self.addEventListener('fetch', (event) => {
  const { request } = event;
  if (request.method !== 'GET') return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin || isServiceRequest(url) || request.headers.get('accept')?.includes('text/event-stream')) return;
  if (request.mode === 'navigate') {
    event.respondWith(networkFirstPage(request));
    return;
  }
  if (isStaticRequest(request, url)) event.respondWith(cacheFirstStatic(request));
});
