import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { DatabaseSync } from 'node:sqlite';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';

let child: ChildProcessWithoutNullStreams | undefined;
let fixtureRoot: string | undefined;

async function availablePort() {
  const server = createServer();
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('failed to allocate a test port');
  await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  return address.port;
}

async function waitForHealth(url: string, errors: Buffer[]) {
  const deadline = Date.now() + 45_000;
  while (Date.now() < deadline) {
    if (child?.exitCode !== null) {
      throw new Error(`Rust server exited early: ${Buffer.concat(errors).toString('utf8')}`);
    }
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {
      // Compilation and listener startup are expected to take a few seconds on a clean build.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Rust server did not become healthy: ${Buffer.concat(errors).toString('utf8')}`);
}

afterEach(async () => {
  if (child && child.exitCode === null) {
    child.kill();
    await new Promise<void>((resolve) => child!.once('exit', () => resolve()));
  }
  child = undefined;
  if (fixtureRoot) {
    const resolved = path.resolve(fixtureRoot);
    const prefix = `${path.resolve(os.tmpdir())}${path.sep}imail-rust-server-`;
    if (!resolved.startsWith(prefix)) throw new Error(`refusing to remove unsafe fixture path: ${resolved}`);
    await rm(resolved, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
    fixtureRoot = undefined;
  }
});

describe('Rust standalone HTTP service host', () => {
  it('initializes an empty isolated data directory and serves health plus the web client', async () => {
    fixtureRoot = await mkdtemp(path.join(os.tmpdir(), 'imail-rust-server-'));
    const dataDir = path.join(fixtureRoot, 'data');
    const webDir = path.join(fixtureRoot, 'web');
    await mkdir(webDir);
    await writeFile(path.join(webDir, 'index.html'), '<!doctype html><title>Rust server host</title>');
    const controlFile = path.join(fixtureRoot, 'daemon-control-token');
    await writeFile(controlFile, 'rust-daemon-control-secret\n');
    const port = await availablePort();
    const errors: Buffer[] = [];
    child = spawn('cargo', [
      'run', '--quiet', '-p', 'imail-http-service',
      '--bin', 'imail-server', '--', '--data-dir', dataDir, '--daemon-control-file', controlFile,
    ], {
      cwd: path.resolve(import.meta.dirname, '..'),
      env: {
        ...process.env,
        HOST: '127.0.0.1',
        PORT: String(port),
        CORS_ORIGIN: 'https://client.example.test',
        IMAIL_ALLOWED_HOSTS: '127.0.0.1,localhost',
        IMAIL_GATEWAY: 'true',
        IMAIL_MCP: 'true',
        IMAIL_REGISTRATION_MODE: 'open',
        IMAIL_TRUST_PROXY: 'true',
        IMAIL_WEB_DIST: webDir,
      },
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
    });
    child.stderr.on('data', (chunk: Buffer) => errors.push(chunk));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForHealth(`${baseUrl}/api/health`, errors);

    const database = new DatabaseSync(path.join(dataDir, 'imail.sqlite'), { timeout: 5_000 });
    try {
      const deadline = Date.now() + 5_000;
      while (Date.now() < deadline) {
        const row = database.prepare('SELECT count(*) AS count FROM sync_worker_heartbeats').get() as { count: number };
        if (row.count === 3) break;
        await new Promise((resolve) => setTimeout(resolve, 25));
      }
      expect(database.prepare('SELECT count(*) AS count FROM sync_worker_heartbeats').get()).toEqual({ count: 3 });
    } finally {
      database.close();
    }

    const info = await fetch(`${baseUrl}/api/system/info`).then((response) => response.json());
    expect(info.capabilities).toMatchObject({ gateway: true, mcp: true, syncWorker: true, webClient: true });
    const route = await fetch(`${baseUrl}/settings`, { headers: { accept: 'text/html' } });
    expect(route.status).toBe(200);
    expect(await route.text()).toContain('Rust server host');
    expect(route.headers.get('content-security-policy')).toContain("frame-ancestors 'none'");

    const preflight = await fetch(`${baseUrl}/api/auth/session`, {
      method: 'OPTIONS',
      headers: {
        origin: 'https://client.example.test',
        'access-control-request-method': 'GET',
      },
    });
    expect(preflight.status).toBe(204);
    expect(preflight.headers.get('access-control-allow-origin')).toBe('https://client.example.test');

    const firstRegistration = await fetch(`${baseUrl}/api/auth/register`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'x-forwarded-for': '198.51.100.20',
        'x-forwarded-proto': 'https',
      },
      body: JSON.stringify({ login: 'legacy-env-owner', displayName: 'Legacy Env Owner', password: 'legacy-env-password-123' }),
    });
    expect(firstRegistration.status).toBe(201);
    expect(firstRegistration.headers.get('strict-transport-security')).toBe('max-age=31536000');
    expect(firstRegistration.headers.get('set-cookie')).toContain('Secure');

    const secondRegistration = await fetch(`${baseUrl}/api/auth/register`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ login: 'legacy-env-second', displayName: 'Legacy Env Second', password: 'legacy-env-password-456' }),
    });
    expect(secondRegistration.status).toBe(201);

    const rejectedShutdown = await fetch(`${baseUrl}/api/system/shutdown`, {
      method: 'POST',
      headers: { 'x-imail-daemon-token': 'wrong-token' },
    });
    expect(rejectedShutdown.status).toBe(403);
    const acceptedShutdown = await fetch(`${baseUrl}/api/system/shutdown`, {
      method: 'POST',
      headers: { 'x-imail-daemon-token': 'rust-daemon-control-secret' },
    });
    expect(acceptedShutdown.status).toBe(202);
    expect(await acceptedShutdown.json()).toEqual({ stopping: true });
    await new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Rust daemon shutdown timed out')), 15_000);
      child!.once('exit', (code) => {
        clearTimeout(timeout);
        code === 0 ? resolve() : reject(new Error(`Rust daemon exited with code ${code}`));
      });
    });
    const stoppedDatabase = new DatabaseSync(path.join(dataDir, 'imail.sqlite'));
    try {
      expect(stoppedDatabase.prepare('SELECT count(*) AS count FROM sync_worker_heartbeats').get()).toEqual({ count: 0 });
    } finally {
      stoppedDatabase.close();
    }
  }, 60_000);
});
