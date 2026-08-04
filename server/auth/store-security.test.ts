import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { afterEach, describe, expect, it } from 'vitest';
import { AuthStore } from './store.js';

let directory = '';

afterEach(async () => {
  if (directory) await rm(directory, { recursive: true, force: true });
  directory = '';
});

describe('persistent authentication security state', () => {
  it('keeps rate limits across store restarts and records redacted audit actors', async () => {
    directory = await mkdtemp(path.join(tmpdir(), 'imail-auth-security-'));
    const databasePath = path.join(directory, 'imail.sqlite');
    let store = new AuthStore(databasePath);
    expect(store.consumeAttempt('login-ip:192.0.2.1', 2, 60_000).allowed).toBe(true);
    expect(store.consumeAttempt('login-ip:192.0.2.1', 2, 60_000).allowed).toBe(true);
    store.recordSecurityEvent('login.failed', '192.0.2.1');
    store.recordSecurityEvent('developer-token.created', '192.0.2.1', 'user-a', { tokenId: 'token-a', scopes: 'mcp:full' });
    store.recordSecurityEvent('developer-token.created', '192.0.2.2', 'user-b', { tokenId: 'token-b' });
    const firstUserEvent = store.listSecurityEvents('user-a', 20)[0];
    expect(store.listSecurityEvents('user-a', 20)).toEqual([
      expect.objectContaining({ eventType: 'developer-token.created', detail: { tokenId: 'token-a', scopes: 'mcp:full' } }),
    ]);
    store.close();

    store = new AuthStore(databasePath);
    expect(store.consumeAttempt('login-ip:192.0.2.1', 2, 60_000).allowed).toBe(false);
    store.recordSecurityEvent('login.succeeded', '192.0.2.1', 'user-a');
    store.recordSecurityEvent('login.succeeded', '192.0.2.3', 'user-a');
    const restartedEvents = store.listSecurityEvents('user-a', 20);
    expect(restartedEvents.find((event) => event.actorHash === firstUserEvent.actorHash)).toBeDefined();
    expect(new Set(restartedEvents.map((event) => event.actorHash)).size).toBe(2);
    store.close();

    const database = new DatabaseSync(databasePath, { readOnly: true });
    const event = database.prepare("SELECT actor_hash, detail_json FROM security_audit_events WHERE event_type = 'login.failed'").get() as { actor_hash: string; detail_json: string };
    const salt = (database.prepare("SELECT value FROM metadata WHERE key = 'security_audit_actor_salt'").get() as { value: string }).value;
    database.close();
    expect(event.actor_hash).toMatch(/^[0-9a-f]{64}$/);
    expect(event.actor_hash).not.toContain('192.0.2.1');
    expect(event.detail_json).toBe('{}');
    expect(salt).toMatch(/^[0-9a-f]{64}$/);
  });
});
