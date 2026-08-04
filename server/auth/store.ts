import { createHash, createHmac, randomBytes, randomUUID } from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';
import { configuredDatabasePath } from '../store.js';
import { ensureSchema } from '../storage/schema.js';
import { hashPassword, verifyPassword } from './password.js';
import { userMetadataKey } from './context.js';

export type AppUser = { id: string; login: string; displayName: string; createdAt: string };
export type SecurityAuditEvent = {
  id: string;
  eventType: string;
  actorHash: string;
  detail: Record<string, string>;
  createdAt: string;
};
const SESSION_TTL_MS = 30 * 24 * 60 * 60_000;
const AUDIT_RETENTION_MS = 90 * 24 * 60 * 60_000;
const MAX_AUDIT_EVENTS = 10_000;

function digest(value: string) { return createHash('sha256').update(value).digest('hex'); }

export class AuthStore {
  private readonly db: DatabaseSync;
  private readonly auditActorSalt: string;
  constructor(databasePath = configuredDatabasePath()) {
    this.db = new DatabaseSync(databasePath, { timeout: 5_000 });
    this.db.exec('PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;');
    ensureSchema(this.db);
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS app_users (
        id TEXT PRIMARY KEY, login TEXT NOT NULL COLLATE NOCASE UNIQUE, display_name TEXT NOT NULL,
        password_hash TEXT NOT NULL, created_at TEXT NOT NULL
      ) STRICT;
      CREATE TABLE IF NOT EXISTS app_sessions (
        id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
        token_hash TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL, expires_at TEXT NOT NULL, last_seen_at TEXT NOT NULL
      ) STRICT;
      CREATE INDEX IF NOT EXISTS app_sessions_expiry ON app_sessions(expires_at);
      CREATE TABLE IF NOT EXISTS auth_rate_limits (
        key_hash TEXT PRIMARY KEY, attempt_count INTEGER NOT NULL, reset_at TEXT NOT NULL
      ) STRICT;
      CREATE INDEX IF NOT EXISTS auth_rate_limits_expiry ON auth_rate_limits(reset_at);
      CREATE TABLE IF NOT EXISTS security_audit_events (
        id TEXT PRIMARY KEY, event_type TEXT NOT NULL, user_id TEXT,
        actor_hash TEXT NOT NULL, detail_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL
      ) STRICT;
      CREATE INDEX IF NOT EXISTS security_audit_events_created ON security_audit_events(created_at DESC);
    `);
    this.db.prepare("INSERT INTO metadata (key, value) VALUES ('security_audit_actor_salt', ?) ON CONFLICT(key) DO NOTHING")
      .run(randomBytes(32).toString('hex'));
    this.auditActorSalt = String((this.db.prepare("SELECT value FROM metadata WHERE key = 'security_audit_actor_salt'").get() as { value: string }).value);
  }

  setupRequired() {
    const row = this.db.prepare('SELECT count(*) AS total FROM app_users').get() as { total: number };
    return Number(row.total) === 0;
  }

  async createUser(input: { login: string; displayName: string; password: string }) {
    const user: AppUser = { id: randomUUID(), login: input.login.toLowerCase(), displayName: input.displayName, createdAt: new Date().toISOString() };
    const passwordHash = await hashPassword(input.password);
    this.db.exec('BEGIN IMMEDIATE');
    try {
      this.db.prepare('INSERT INTO app_users (id, login, display_name, password_hash, created_at) VALUES (?, ?, ?, ?, ?)')
        .run(user.id, user.login, user.displayName, passwordHash, user.createdAt);
      if (this.db.prepare('SELECT count(*) AS total FROM app_users').get() && Number((this.db.prepare('SELECT count(*) AS total FROM app_users').get() as { total: number }).total) === 1) {
        for (const table of ['accounts', 'developer_tokens', 'contacts', 'logo_fetch_attempts']) {
          this.db.prepare(`UPDATE ${table} SET user_id = ? WHERE user_id = '__legacy__'`).run(user.id);
        }
        this.db.prepare(`UPDATE metadata SET key = ? WHERE key = 'app_preferences_v1'
          AND NOT EXISTS (SELECT 1 FROM metadata WHERE key = ?)`).run(userMetadataKey('app_preferences_v1', user.id), userMetadataKey('app_preferences_v1', user.id));
      }
      this.db.exec('COMMIT');
      return user;
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }

  async authenticate(login: string, password: string) {
    const row = this.db.prepare('SELECT * FROM app_users WHERE login = ? COLLATE NOCASE').get(login) as Record<string, string> | undefined;
    if (!row || !await verifyPassword(password, row.password_hash)) return null;
    return { id: row.id, login: row.login, displayName: row.display_name, createdAt: row.created_at } satisfies AppUser;
  }

  consumeAttempt(key: string, maximum: number, windowMs: number) {
    const keyHash = digest(key);
    const now = new Date();
    const resetAt = new Date(now.getTime() + windowMs);
    this.db.exec('BEGIN IMMEDIATE');
    try {
      this.db.prepare('DELETE FROM auth_rate_limits WHERE reset_at <= ?').run(now.toISOString());
      const row = this.db.prepare('SELECT attempt_count, reset_at FROM auth_rate_limits WHERE key_hash = ?').get(keyHash) as { attempt_count: number; reset_at: string } | undefined;
      if (row && row.attempt_count >= maximum) {
        this.db.exec('COMMIT');
        return { allowed: false, retryAfter: Math.max(1, Math.ceil((new Date(row.reset_at).getTime() - now.getTime()) / 1_000)) };
      }
      if (row) this.db.prepare('UPDATE auth_rate_limits SET attempt_count = attempt_count + 1 WHERE key_hash = ?').run(keyHash);
      else {
        const total = Number((this.db.prepare('SELECT count(*) AS total FROM auth_rate_limits').get() as { total: number }).total);
        if (total >= 10_000) this.db.prepare('DELETE FROM auth_rate_limits WHERE key_hash IN (SELECT key_hash FROM auth_rate_limits ORDER BY reset_at LIMIT 100)').run();
        this.db.prepare('INSERT INTO auth_rate_limits (key_hash, attempt_count, reset_at) VALUES (?, 1, ?)').run(keyHash, resetAt.toISOString());
      }
      this.db.exec('COMMIT');
      return { allowed: true, retryAfter: 0 };
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }

  clearAttempt(key: string) {
    this.db.prepare('DELETE FROM auth_rate_limits WHERE key_hash = ?').run(digest(key));
  }

  recordSecurityEvent(eventType: string, actor: string, userId?: string, detail: Record<string, string> = {}) {
    const now = new Date();
    this.db.exec('BEGIN IMMEDIATE');
    try {
      this.db.prepare('DELETE FROM security_audit_events WHERE created_at < ?')
        .run(new Date(now.getTime() - AUDIT_RETENTION_MS).toISOString());
      this.db.prepare('INSERT INTO security_audit_events (id, event_type, user_id, actor_hash, detail_json, created_at) VALUES (?, ?, ?, ?, ?, ?)')
        .run(randomUUID(), eventType, userId ?? null, createHmac('sha256', this.auditActorSalt).update(actor || 'unknown').digest('hex'), JSON.stringify(detail), now.toISOString());
      const total = Number((this.db.prepare('SELECT count(*) AS total FROM security_audit_events').get() as { total: number }).total);
      if (total > MAX_AUDIT_EVENTS) {
        this.db.prepare('DELETE FROM security_audit_events WHERE id IN (SELECT id FROM security_audit_events ORDER BY created_at ASC, id ASC LIMIT ?)')
          .run(total - MAX_AUDIT_EVENTS);
      }
      this.db.exec('COMMIT');
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }

  listSecurityEvents(userId: string, limit = 100): SecurityAuditEvent[] {
    const boundedLimit = Math.max(1, Math.min(500, Math.trunc(limit)));
    const rows = this.db.prepare(`SELECT id, event_type, actor_hash, detail_json, created_at
      FROM security_audit_events WHERE user_id = ? ORDER BY created_at DESC, id DESC LIMIT ?`)
      .all(userId, boundedLimit) as Array<Record<string, string>>;
    return rows.map((row) => {
      let detail: Record<string, string> = {};
      try {
        const parsed = JSON.parse(row.detail_json) as unknown;
        if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
          detail = Object.fromEntries(Object.entries(parsed).filter((entry): entry is [string, string] => typeof entry[1] === 'string'));
        }
      } catch { /* Treat malformed legacy detail as empty instead of failing the audit endpoint. */ }
      return { id: row.id, eventType: row.event_type, actorHash: row.actor_hash, detail, createdAt: row.created_at };
    });
  }

  createSession(userId: string) {
    const raw = randomBytes(32).toString('base64url');
    const now = new Date();
    this.db.prepare('DELETE FROM app_sessions WHERE expires_at <= ?').run(now.toISOString());
    this.db.prepare('INSERT INTO app_sessions (id, user_id, token_hash, created_at, expires_at, last_seen_at) VALUES (?, ?, ?, ?, ?, ?)')
      .run(randomUUID(), userId, digest(raw), now.toISOString(), new Date(now.getTime() + SESSION_TTL_MS).toISOString(), now.toISOString());
    return raw;
  }

  userForSession(raw: string | undefined) {
    if (!raw) return null;
    const row = this.db.prepare(`SELECT u.id, u.login, u.display_name, u.created_at, s.id AS session_id
      FROM app_sessions s JOIN app_users u ON u.id = s.user_id WHERE s.token_hash = ? AND s.expires_at > ?`)
      .get(digest(raw), new Date().toISOString()) as Record<string, string> | undefined;
    if (!row) return null;
    this.db.prepare('UPDATE app_sessions SET last_seen_at = ? WHERE id = ?').run(new Date().toISOString(), row.session_id);
    return { id: row.id, login: row.login, displayName: row.display_name, createdAt: row.created_at } satisfies AppUser;
  }

  deleteSession(raw: string | undefined) { if (raw) this.db.prepare('DELETE FROM app_sessions WHERE token_hash = ?').run(digest(raw)); }
  close() { this.db.close(); }
}
