import { readFile, readdir, stat } from 'node:fs/promises';
import path from 'node:path';

const assetDirectory = path.resolve(import.meta.dirname, '..', 'frontend', 'dist', 'assets');
const assets = await Promise.all((await readdir(assetDirectory)).map(async (name) => {
  const bytes = (await stat(path.join(assetDirectory, name))).size;
  return { name, bytes };
}));
const javascript = assets.filter(({ name }) => name.endsWith('.js'));
const totalBytes = javascript.reduce((sum, asset) => sum + asset.bytes, 0);
const largest = [...javascript].sort((left, right) => right.bytes - left.bytes)[0];
const html = await readFile(path.resolve(assetDirectory, '..', 'index.html'), 'utf8');
const initialNames = [...html.matchAll(/(?:src|href)="\/assets\/([^"]+\.js)"/g)].map((match) => match[1]);
const initialAssets = javascript.filter(({ name }) => initialNames.includes(name));
const initialBytes = initialAssets.reduce((sum, asset) => sum + asset.bytes, 0);
const kib = (bytes) => `${(bytes / 1024).toFixed(1)} KiB`;

console.log(`Frontend JavaScript: ${javascript.length} chunks, ${kib(totalBytes)} total, largest ${largest?.name ?? 'n/a'} (${kib(largest?.bytes ?? 0)})`);
console.log(`Initial JavaScript: ${initialAssets.length} chunks, ${kib(initialBytes)} total`);

if (totalBytes > 1_400 * 1024) throw new Error(`JavaScript total exceeds 1400 KiB: ${kib(totalBytes)}`);
if (largest && largest.bytes > 450 * 1024) throw new Error(`Chunk exceeds 450 KiB: ${largest.name} (${kib(largest.bytes)})`);
if (initialBytes > 900 * 1024) throw new Error(`Initial JavaScript exceeds 900 KiB: ${kib(initialBytes)}`);
if (initialNames.some((name) => name.startsWith('editor-vendor-'))) throw new Error('Editor vendor must not be preloaded on the initial page');
