import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { request as nodeRequest } from 'node:http';
import WebSocket from 'ws';
import { DatabaseSync } from 'node:sqlite';
import { randomUUID } from 'node:crypto';

const root = path.resolve(import.meta.dirname, '..');
const port = Number(process.env.IMAIL_REMOTE_SMOKE_PORT || 18788);
const publicHost = 'mail.example.test';
const publicOrigin = `https://${publicHost}`;
const baseUrl = `http://127.0.0.1:${port}`;
const dataDir = await mkdtemp(path.join(tmpdir(), 'imail-remote-smoke-'));
const maintenanceDir = await mkdtemp(path.join(tmpdir(), 'imail-remote-maintenance-'));
const execFileAsync = promisify(execFile);
const child = spawn(process.execPath, ['server-runtime/imail-server.cjs'], {
  cwd: root,
  stdio: ['ignore', 'pipe', 'pipe'],
  windowsHide: true,
  env: {
    ...process.env,
    NODE_ENV: 'production',
    HOST: '127.0.0.1', PORT: String(port), IMAIL_DATA_DIR: dataDir,
    IMAIL_SYNC_WORKER_MODE: 'disabled', IMAIL_TRUST_PROXY: 'true',
    IMAIL_ALLOWED_HOSTS: `${publicHost},127.0.0.1`, MCP_ALLOWED_HOSTS: `${publicHost},127.0.0.1`,
    CORS_ORIGIN: publicOrigin, FRONTEND_URL: publicOrigin,
    OAUTH_CALLBACK_BASE_URL: `${publicOrigin}/api/oauth`, IMAIL_REGISTRATION_MODE: 'initial-only',
  },
});
let stderr = '';
child.stderr.on('data', (chunk) => { stderr += chunk; });

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const publicHeaders = { Host: publicHost, Origin: publicOrigin, 'X-Forwarded-Proto': 'https' };

async function waitFor(check, description, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try { const result = await check(); if (result) return result; } catch (error) { lastError = error; }
    await delay(200);
  }
  throw new Error(`${description}超时${lastError ? `：${lastError}` : ''}\n${stderr}`);
}

async function request(route, init = {}) {
  const headers = { ...publicHeaders, ...Object.fromEntries(new Headers(init.headers).entries()) };
  const body = init.body ? String(init.body) : undefined;
  if (body) headers['Content-Length'] = String(Buffer.byteLength(body));
  return new Promise((resolve, reject) => {
    const outgoing = nodeRequest({ hostname: '127.0.0.1', port, path: route, method: init.method || 'GET', headers }, (incoming) => {
      const chunks = [];
      incoming.on('data', (chunk) => chunks.push(chunk));
      incoming.once('end', () => {
        const responseBody = Buffer.concat(chunks).toString('utf8');
        const responseHeaders = new Headers();
        for (const [name, value] of Object.entries(incoming.headers)) {
          if (Array.isArray(value)) for (const item of value) responseHeaders.append(name, item);
          else if (value !== undefined) responseHeaders.set(name, value);
        }
        resolve({
          status: incoming.statusCode || 0,
          ok: (incoming.statusCode || 0) >= 200 && (incoming.statusCode || 0) < 300,
          headers: responseHeaders,
          text: async () => responseBody,
          json: async () => JSON.parse(responseBody),
        });
      });
    });
    outgoing.once('error', reject);
    if (body) outgoing.write(body);
    outgoing.end();
  });
}

function firstSseEvent(cookie) {
  return new Promise((resolve, reject) => {
    const outgoing = nodeRequest({
      hostname: '127.0.0.1', port, path: '/api/events',
      headers: { ...publicHeaders, Cookie: cookie, Accept: 'text/event-stream' },
    }, (incoming) => {
      if (incoming.statusCode !== 200) { incoming.resume(); reject(new Error(`SSE HTTP ${incoming.statusCode}`)); return; }
      incoming.once('data', (chunk) => { const value = chunk.toString('utf8'); incoming.destroy(); resolve(value); });
    });
    outgoing.once('error', reject);
    outgoing.end();
  });
}

function websocketConnected(token) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(`ws://127.0.0.1:${port}/gateway/v1/events`, {
      headers: { Host: publicHost, Origin: publicOrigin, Authorization: `Bearer ${token}` },
    });
    const timeout = setTimeout(() => { socket.terminate(); reject(new Error('WebSocket 连接超时')); }, 5_000);
    socket.once('message', (data) => {
      const message = JSON.parse(data.toString());
      clearTimeout(timeout); socket.close();
      if (message.type !== 'connected') reject(new Error(`WebSocket 未返回 connected：${data}`));
      else resolve(message);
    });
    socket.once('error', reject);
  });
}

function websocketRejectedForOrigin(token, origin = 'https://attacker.example') {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(`ws://127.0.0.1:${port}/gateway/v1/events`, {
      headers: { Host: publicHost, Origin: origin, Authorization: `Bearer ${token}` },
    });
    const timeout = setTimeout(() => { socket.terminate(); reject(new Error('非法 Origin 未及时拒绝')); }, 5_000);
    socket.once('unexpected-response', (_request, response) => {
      clearTimeout(timeout); response.destroy(); resolve(response.statusCode === 403);
    });
    socket.once('open', () => { clearTimeout(timeout); socket.terminate(); reject(new Error('非法 Origin 建立了 WebSocket')); });
    socket.once('error', () => undefined);
  });
}

try {
  const info = await waitFor(async () => {
    const response = await request('/api/system/info');
    return response.ok ? response.json() : undefined;
  }, '远程运行包启动');
  if (!info.capabilities.webClient) throw new Error('远程运行包未声明 Web 客户端能力');

  const page = await request('/settings', { headers: { Accept: 'text/html' } });
  const html = await page.text();
  if (!page.ok || !html.includes('<div id="root"></div>') || !page.headers.has('content-security-policy')) throw new Error('SPA 或 CSP 验证失败');
  const unknownHost = await request('/api/system/info', { headers: { Host: 'attacker.example', Origin: 'https://attacker.example' } });
  if (unknownHost.status !== 421) throw new Error(`未知 Host 未被拒绝：${unknownHost.status}`);
  const developmentOrigin = await request('/api/system/info', { headers: { Origin: 'http://localhost:5173' } });
  if (developmentOrigin.headers.has('access-control-allow-origin')) throw new Error('生产运行包仍隐式允许 localhost 开发来源');

  const initial = await request('/api/auth/status').then((response) => response.json());
  if (!initial.setupRequired || !initial.registrationOpen) throw new Error('首次初始化状态不正确');
  const registration = await request('/api/auth/register', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ login: 'release-smoke', displayName: 'Release Smoke', password: 'release-smoke-password' }),
  });
  if (registration.status !== 201) throw new Error(`首次注册失败：${registration.status}`);
  const registeredUser = (await registration.json()).user;
  const setCookie = registration.headers.get('set-cookie') || '';
  if (!setCookie.includes('HttpOnly') || !setCookie.includes('SameSite=Lax') || !setCookie.includes('Secure')) throw new Error(`代理 Cookie 属性不正确：${setCookie}`);
  const cookie = setCookie.split(';')[0];
  const closedRegistration = await request('/api/auth/register', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ login: 'second-user', displayName: 'Second', password: 'release-smoke-password' }),
  });
  if (closedRegistration.status !== 403) throw new Error('初始化后注册没有关闭');

  const database = new DatabaseSync(path.join(dataDir, 'imail.sqlite'));
  const accountId = randomUUID();
  database.prepare(`INSERT INTO accounts (
    id, provider, email, display_name, group_name, group_icon, color, settings_json,
    encrypted_secret, auth_method, created_at, status, mailboxes_json, user_id
  ) VALUES (?, 'custom', ?, 'Release Mailbox', 'Smoke', 'folder', '#168f78', ?, ?, 'password', ?, 'connected', '[]', ?)`)
    .run(accountId, 'smoke@example.test', JSON.stringify({ imapHost: 'imap.example.test', imapPort: 993, imapSecure: true, smtpHost: 'smtp.example.test', smtpPort: 465, smtpSecure: true }), 'smoke-never-decrypted', new Date().toISOString(), registeredUser.id);
  database.close();

  const secondLogin = await request('/api/auth/login', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ login: 'release-smoke', password: 'release-smoke-password' }),
  });
  if (!secondLogin.ok) throw new Error(`第二设备登录失败：${secondLogin.status}`);
  const secondCookie = (secondLogin.headers.get('set-cookie') || '').split(';')[0];
  if (!secondCookie || secondCookie === cookie) throw new Error('两个设备没有获得独立会话');

  const firstAccounts = await request('/api/accounts', { headers: { Cookie: cookie } });
  const secondAccounts = await request('/api/accounts', { headers: { Cookie: secondCookie } });
  if (!firstAccounts.ok || !secondAccounts.ok) throw new Error('双设备无法读取同一远程账户');
  if ((await firstAccounts.json()).accounts[0]?.id !== accountId || (await secondAccounts.json()).accounts[0]?.id !== accountId) {
    throw new Error('双设备没有看到同一远程邮箱账户');
  }

  const draftPayload = { accountId, to: ['friend@example.test'], cc: [], subject: 'Created on device A', text: 'Shared draft', html: '', attachments: [] };
  const createdDraftResponse = await request('/api/drafts', {
    method: 'POST', headers: { Cookie: cookie, 'Content-Type': 'application/json' }, body: JSON.stringify(draftPayload),
  });
  if (createdDraftResponse.status !== 201) throw new Error(`设备 A 创建共享草稿失败：${createdDraftResponse.status}`);
  const createdDraft = (await createdDraftResponse.json()).draft;
  const draftsOnSecond = await request('/api/drafts', { headers: { Cookie: secondCookie } }).then((response) => response.json());
  if (draftsOnSecond.drafts[0]?.id !== createdDraft.id || draftsOnSecond.drafts[0]?.subject !== draftPayload.subject) {
    throw new Error('设备 B 没有看到设备 A 创建的草稿');
  }
  const updatedDraftResponse = await request(`/api/drafts/${createdDraft.id}`, {
    method: 'PUT', headers: { Cookie: secondCookie, 'Content-Type': 'application/json' },
    body: JSON.stringify({ ...draftPayload, subject: 'Updated on device B' }),
  });
  if (!updatedDraftResponse.ok) throw new Error(`设备 B 更新共享草稿失败：${updatedDraftResponse.status}`);
  const draftsOnFirst = await request('/api/drafts', { headers: { Cookie: cookie } }).then((response) => response.json());
  if (draftsOnFirst.drafts[0]?.subject !== 'Updated on device B') throw new Error('设备 A 没有看到设备 B 的草稿更新');

  const firstEvent = await firstSseEvent(cookie);
  if (!firstEvent.includes('event: connected')) throw new Error('同源 SSE 未建立');

  const tokenResponse = await request('/api/developer-tokens', {
    method: 'POST', headers: { Cookie: cookie, 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: 'Release WebSocket', scopes: ['messages:read'], mailboxes: ['smoke@example.test'], ttlSeconds: 3600 }),
  });
  if (tokenResponse.status !== 201) throw new Error(`创建 WebSocket Token 失败：${tokenResponse.status}`);
  const tokenPayload = await tokenResponse.json();
  const token = tokenPayload.token;
  const auditResponse = await request('/api/security/audit-events?limit=20', { headers: { Cookie: secondCookie } });
  if (!auditResponse.ok) throw new Error(`安全审计查询失败：${auditResponse.status}`);
  const auditText = await auditResponse.text();
  const auditEvents = JSON.parse(auditText).events;
  if (!auditEvents.some((event) => event.eventType === 'developer-token.created' && event.detail?.tokenId === tokenPayload.detail.id)) {
    throw new Error('打包后远程运行时没有记录授权码管理审计');
  }
  if (auditText.includes(token) || !auditEvents.every((event) => /^[0-9a-f]{64}$/.test(event.actorHash))) {
    throw new Error('安全审计泄露了原始 Token 或未对来源做 HMAC');
  }
  await websocketConnected(token);
  if (!await websocketRejectedForOrigin(token)) throw new Error('WebSocket Origin 拒绝状态不正确');
  if (!await websocketRejectedForOrigin(token, `http://${publicHost}`)) throw new Error('WebSocket 接受了同主机明文 Origin');

  const firstLogout = await request('/api/auth/logout', { method: 'POST', headers: { Cookie: cookie } });
  if (firstLogout.status !== 204) throw new Error(`设备 A 退出失败：${firstLogout.status}`);
  if ((await request('/api/accounts', { headers: { Cookie: cookie } })).status !== 401) throw new Error('设备 A 退出后会话仍然有效');
  const secondAfterLogout = await request('/api/drafts', { headers: { Cookie: secondCookie } });
  if (!secondAfterLogout.ok || (await secondAfterLogout.json()).drafts[0]?.subject !== 'Updated on device B') {
    throw new Error('设备 A 退出错误地终止了设备 B 会话');
  }

  const missingApi = await request('/api/not-a-route', { headers: { Cookie: secondCookie, Accept: 'text/html' } });
  if (missingApi.status !== 404 || (await missingApi.text()).includes('<div id="root"></div>')) throw new Error('SPA fallback 吞掉了 API 404');

  const backupDir = path.join(maintenanceDir, 'backup');
  const preflightDir = path.join(maintenanceDir, 'preflight');
  const maintenance = JSON.parse((await execFileAsync(
    process.execPath,
    ['server-runtime/imail-upgrade-preflight.mjs', backupDir, preflightDir],
    { cwd: root, env: { ...process.env, IMAIL_DATA_DIR: dataDir }, windowsHide: true },
  )).stdout);
  if (!maintenance.activeDataUntouched || !maintenance.integrityManifestVerified || !maintenance.sqliteQuickCheck || !maintenance.foreignKeysVerified) {
    throw new Error('远程运行包升级预检工具失败');
  }

  console.log(JSON.stringify({ ok: true, webClient: true, spa: true, secureProxyCookie: true, registrationClosed: true, productionCorsOnly: true, persistentSecurityAudit: true, multiDeviceSharedData: true, independentSessions: true, sse: true, websocket: true, websocketOriginProtection: true, hostProtection: true, upgradePreflight: true, maintenanceTools: true }));
} finally {
  if (child.exitCode === null) child.kill('SIGTERM');
  await Promise.race([new Promise((resolve) => child.once('exit', resolve)), delay(5_000)]);
  if (child.exitCode === null) child.kill('SIGKILL');
  await rm(dataDir, { recursive: true, force: true });
  await rm(maintenanceDir, { recursive: true, force: true });
}
