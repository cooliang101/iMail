import { Client, StreamableHTTPClientTransport } from '@modelcontextprotocol/client';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { afterEach, describe, expect, it } from 'vitest';

type FixtureConnection = { url: string; token: string };

let child: ChildProcessWithoutNullStreams | undefined;
let fixtureDirectory: string | undefined;

async function startRustFixture(): Promise<FixtureConnection> {
  fixtureDirectory = await mkdtemp(path.join(os.tmpdir(), 'imail-mcp-sdk-'));
  child = spawn('cargo', [
    'run', '--quiet', '-p', 'imail-http',
    '--bin', 'imail-mcp-fixture-server', '--', '--data-dir', fixtureDirectory,
  ], {
    cwd: path.resolve(import.meta.dirname, '..'),
    env: process.env,
    stdio: ['pipe', 'pipe', 'pipe'],
    windowsHide: true,
  });
  const errors: Buffer[] = [];
  child.stderr.on('data', (chunk: Buffer) => errors.push(chunk));
  const lines = readline.createInterface({ input: child.stdout });
  return await new Promise<FixtureConnection>((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error(`Rust MCP fixture did not start: ${Buffer.concat(errors).toString('utf8')}`));
    }, 45_000);
    child!.once('error', (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    child!.once('exit', (code) => {
      clearTimeout(timeout);
      reject(new Error(`Rust MCP fixture exited with ${code}: ${Buffer.concat(errors).toString('utf8')}`));
    });
    lines.once('line', (line) => {
      clearTimeout(timeout);
      try {
        const connection = JSON.parse(line) as FixtureConnection;
        if (!connection.url.startsWith('http://127.0.0.1:') || !connection.token.startsWith('imail_mcp_')) {
          throw new Error('fixture emitted an unsafe or incomplete connection descriptor');
        }
        resolve(connection);
      } catch (error) {
        reject(error);
      }
    });
  });
}

afterEach(async () => {
  child?.kill();
  child = undefined;
  if (fixtureDirectory) {
    const resolved = path.resolve(fixtureDirectory);
    const expectedPrefix = `${path.resolve(os.tmpdir())}${path.sep}imail-mcp-sdk-`;
    if (!resolved.startsWith(expectedPrefix)) throw new Error(`refusing to remove unsafe fixture path: ${resolved}`);
    await rm(resolved, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
    fixtureDirectory = undefined;
  }
});

describe('official TypeScript SDK against the Rust MCP transport', () => {
  it('negotiates 2026-07-28, discovers the shared tools and calls a Rust service', async () => {
    const connection = await startRustFixture();
    const transport = new StreamableHTTPClientTransport(new URL(connection.url), {
      requestInit: { headers: { authorization: `Bearer ${connection.token}` } },
    });
    const client = new Client(
      { name: 'imail-rust-interop', version: '1.0.0' },
      { versionNegotiation: { mode: 'auto' } },
    );
    try {
      await client.connect(transport);
      expect(client.getServerVersion()).toMatchObject({ name: 'imail', version: '1.0.0' });
      expect(client.getServerCapabilities()).toMatchObject({ tools: { listChanged: true } });
      const listed = await client.listTools();
      expect(listed.tools).toHaveLength(38);
      expect(listed.tools.map((tool) => tool.name)).toContain('imail_status');
      const status = await client.callTool({ name: 'imail_status', arguments: {} });
      expect(status.isError).not.toBe(true);
      expect(status.structuredContent).toMatchObject({ accounts: 0, messages: 0, drafts: 0 });
    } finally {
      await client.close();
    }
  }, 60_000);
});
