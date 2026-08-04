import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import type { Server } from 'node:http';
import { afterEach, describe, expect, it } from 'vitest';
import { createApp } from './app.js';

let directory = '';
let server: Server | undefined;

afterEach(async () => {
  if (server) await new Promise<void>((resolve, reject) => server?.close((error) => error ? reject(error) : resolve()));
  server = undefined;
  if (directory) await rm(directory, { recursive: true, force: true });
  directory = '';
});

async function start() {
  directory = await mkdtemp(path.join(tmpdir(), 'imail-web-'));
  await writeFile(path.join(directory, 'index.html'), '<!doctype html><title>iMail hosted web</title>');
  await writeFile(path.join(directory, 'asset.txt'), 'asset');
  server = createApp({ webRoot: directory }).listen(0, '127.0.0.1');
  await new Promise<void>((resolve) => server?.once('listening', resolve));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('测试服务器启动失败');
  return `http://127.0.0.1:${address.port}`;
}

describe('hosted web client', () => {
  it('serves assets and uses the application shell for browser routes', async () => {
    const baseUrl = await start();
    const asset = await fetch(`${baseUrl}/asset.txt`);
    expect(asset.status).toBe(200);
    expect(await asset.text()).toBe('asset');
    const route = await fetch(`${baseUrl}/settings`, { headers: { Accept: 'text/html' } });
    expect(route.status).toBe(200);
    expect(await route.text()).toContain('iMail hosted web');
    expect(route.headers.get('cache-control')).toBe('no-cache');
  });

  it('advertises the web capability without swallowing reserved endpoints', async () => {
    const baseUrl = await start();
    const info = await fetch(`${baseUrl}/api/system/info`).then((response) => response.json());
    expect(info.capabilities.webClient).toBe(true);
    const missingApi = await fetch(`${baseUrl}/api/not-a-route`, { headers: { Accept: 'text/html' } });
    expect(missingApi.status).toBe(401);
    expect(await missingApi.text()).not.toContain('iMail hosted web');
  });
});
