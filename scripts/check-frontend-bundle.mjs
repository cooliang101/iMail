import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { gzipSync } from 'node:zlib';

const assetDirectory = path.resolve(import.meta.dirname, '..', 'frontend', 'dist', 'assets');
const assets = await Promise.all((await readdir(assetDirectory)).map(async (name) => {
  const contents = await readFile(path.join(assetDirectory, name));
  return { name, bytes: contents.byteLength, gzipBytes: gzipSync(contents).byteLength };
}));
const javascript = assets.filter(({ name }) => name.endsWith('.js'));
const stylesheets = assets.filter(({ name }) => name.endsWith('.css'));
const totalBytes = javascript.reduce((sum, asset) => sum + asset.bytes, 0);
const totalGzipBytes = javascript.reduce((sum, asset) => sum + asset.gzipBytes, 0);
const largest = [...javascript].sort((left, right) => right.bytes - left.bytes)[0];
const html = await readFile(path.resolve(assetDirectory, '..', 'index.html'), 'utf8');
const initialNames = [...html.matchAll(/(?:src|href)="\/assets\/([^"]+\.js)"/g)].map((match) => match[1]);
const initialStyleNames = [...html.matchAll(/href="\/assets\/([^"]+\.css)"/g)].map((match) => match[1]);
const initialAssets = javascript.filter(({ name }) => initialNames.includes(name));
const initialStyles = stylesheets.filter(({ name }) => initialStyleNames.includes(name));
const initialBytes = initialAssets.reduce((sum, asset) => sum + asset.bytes, 0);
const initialGzipBytes = initialAssets.reduce((sum, asset) => sum + asset.gzipBytes, 0);
const initialStyleBytes = initialStyles.reduce((sum, asset) => sum + asset.bytes, 0);
const initialStyleGzipBytes = initialStyles.reduce((sum, asset) => sum + asset.gzipBytes, 0);
const kib = (bytes) => `${(bytes / 1024).toFixed(1)} KiB`;

console.log(`Frontend JavaScript: ${javascript.length} chunks, ${kib(totalBytes)} raw / ${kib(totalGzipBytes)} gzip, largest ${largest?.name ?? 'n/a'} (${kib(largest?.bytes ?? 0)})`);
console.log(`Initial JavaScript: ${initialAssets.length} chunks, ${kib(initialBytes)} raw / ${kib(initialGzipBytes)} gzip`);
console.log(`Initial CSS: ${initialStyles.length} chunks, ${kib(initialStyleBytes)} raw / ${kib(initialStyleGzipBytes)} gzip`);

if (totalBytes > 1_400 * 1024) throw new Error(`JavaScript total exceeds 1400 KiB: ${kib(totalBytes)}`);
if (largest && largest.bytes > 450 * 1024) throw new Error(`Chunk exceeds 450 KiB: ${largest.name} (${kib(largest.bytes)})`);
if (initialBytes > 650 * 1024) throw new Error(`Initial JavaScript exceeds 650 KiB: ${kib(initialBytes)}`);
if (initialGzipBytes > 190 * 1024) throw new Error(`Initial JavaScript gzip exceeds 190 KiB: ${kib(initialGzipBytes)}`);
if (initialStyleBytes > 100 * 1024) throw new Error(`Initial CSS exceeds 100 KiB: ${kib(initialStyleBytes)}`);
if (initialStyleGzipBytes > 20 * 1024) throw new Error(`Initial CSS gzip exceeds 20 KiB: ${kib(initialStyleGzipBytes)}`);
if (initialNames.some((name) => name.startsWith('editor-vendor-'))) throw new Error('Editor vendor must not be preloaded on the initial page');
