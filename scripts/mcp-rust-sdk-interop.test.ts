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
  const cargoTargetArguments = process.platform === 'win32'
    ? ['--target', 'x86_64-pc-windows-msvc']
    : [];
  child = spawn('cargo', [
    'run', '--quiet', ...cargoTargetArguments, '-p', 'imail-http',
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
      expect(listed.tools).toHaveLength(63);
      expect(listed.tools.map((tool) => tool.name)).toContain('conversation_get');
      expect(listed.tools.map((tool) => tool.name)).toContain('imail_status');
      expect(listed.tools.map((tool) => tool.name)).toContain('outbox_schedule');
      expect(listed.tools.map((tool) => tool.name)).toContain('outbox_resolve');
      expect(listed.tools.map((tool) => tool.name)).toContain('translation_profiles_list');
      expect(listed.tools.map((tool) => tool.name)).toContain('message_translate');
      expect(listed.tools.map((tool) => tool.name)).toContain('mail_work_items_list');
      expect(listed.tools.map((tool) => tool.name)).toContain('mail_reply_draft_create');
      expect(listed.tools.map((tool) => tool.name)).toContain('mail_draft_schedule');
      const status = await client.callTool({ name: 'imail_status', arguments: {} });
      expect(status.isError).not.toBe(true);
      expect(status.structuredContent).toMatchObject({ accounts: 0, messages: 0, drafts: 0 });
      const saved = await client.callTool({ name: 'smart_folder_save', arguments: { name: '项目预算', filters: { body: '季度预算', unread: false } } });
      expect(saved.isError).not.toBe(true);
      const folder = (saved.structuredContent as { folder: { id: string; filters: { body: string; unread: boolean } } }).folder;
      expect(folder.filters).toMatchObject({ body: '季度预算', unread: false });
      const folders = await client.callTool({ name: 'smart_folders_list', arguments: {} });
      expect(folders.structuredContent).toMatchObject({ folders: [{ id: folder.id }] });
      const results = await client.callTool({ name: 'messages_list', arguments: { filters: folder.filters } });
      expect(results.isError).not.toBe(true);
      const removed = await client.callTool({ name: 'smart_folder_delete', arguments: { folderId: folder.id } });
      expect(removed.structuredContent).toMatchObject({ deleted: true });
      const ruleInput = { name: '开发通知', enabled: false, priority: 100, accountIds: [], matchMode: 'all', conditions: [{ field: 'senderDomain', value: 'github.com' }], actions: [{ type: 'addLabel', value: '开发通知' }], stopProcessing: false };
      const ruleSaved = await client.callTool({ name: 'mail_rule_save', arguments: { input: ruleInput } });
      expect(ruleSaved.isError).not.toBe(true);
      const rule = (ruleSaved.structuredContent as { rule: { id: string } }).rule;
      const rules = await client.callTool({ name: 'mail_rules_list', arguments: {} });
      expect(rules.structuredContent).toMatchObject({ rules: [{ id: rule.id, enabled: false }] });
      const fetchedRule = await client.callTool({ name: 'mail_rule_get', arguments: { ruleId: rule.id } });
      expect(fetchedRule.structuredContent).toMatchObject({ rule: { id: rule.id, enabled: false } });
      const enabledRule = await client.callTool({ name: 'mail_rule_set_enabled', arguments: { ruleId: rule.id, enabled: true } });
      expect(enabledRule.structuredContent).toMatchObject({ rule: { id: rule.id, enabled: true } });
      const disabledRule = await client.callTool({ name: 'mail_rule_set_enabled', arguments: { ruleId: rule.id, enabled: false } });
      expect(disabledRule.structuredContent).toMatchObject({ rule: { id: rule.id, enabled: false } });
      const preview = await client.callTool({ name: 'mail_rule_preview', arguments: { ruleId: rule.id, input: ruleInput } });
      expect(preview.isError).not.toBe(true);
      const token = (preview.structuredContent as { token: string }).token;
      expect(token).toBeTypeOf('string');
      expect(preview.structuredContent).toMatchObject({ total: 0, eligible: 0 });
      const applied = await client.callTool({ name: 'mail_rule_apply', arguments: { token, confirmed: true } });
      expect(applied.structuredContent).toMatchObject({ queued: 0 });
      const repeated = await client.callTool({ name: 'mail_rule_apply', arguments: { token, confirmed: true } });
      expect(repeated.isError).toBe(true);
      const runs = await client.callTool({ name: 'mail_rule_runs', arguments: {} });
      expect(runs.structuredContent).toMatchObject({ runs: [] });
      const missingRun = await client.callTool({ name: 'mail_rule_retry', arguments: { runId: 'not-owned' } });
      expect(missingRun.isError).toBe(true);
      const ruleRemoved = await client.callTool({ name: 'mail_rule_delete', arguments: { ruleId: rule.id } });
      expect(ruleRemoved.structuredContent).toMatchObject({ deleted: true });
    } finally {
      await client.close();
    }
  }, 60_000);
});
