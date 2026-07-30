import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';
import { configuredDatabasePath } from '../store.js';
import { ensureSchema } from '../storage/schema.js';
import { hashPassword, verifyPassword } from './password.js';
import { userMetadataKey } from './context.js';

export type AppUser = { id: string; login: string; displayName: string; createdAt: string };
const SESSION_TTL_MS = 30 * 24 * 60 * 60_000;

function digest(value: string) { return createHash('sha256').update(value).digest('hex'); }

export class AuthStore {
  private readonly db: DatabaseSync;
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
    `);
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
