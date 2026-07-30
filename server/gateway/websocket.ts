import type { Server, IncomingMessage } from 'node:http';
import type { Duplex } from 'node:stream';
import WebSocket, { WebSocketServer } from 'ws';
import { authenticateToken } from '../tokens.js';
import type { DeveloperToken } from '../types.js';
import { gatewayEvents, type GatewayMessageCreatedEvent } from './events.js';
import { getSyncStore } from '../sync/store.js';

const EVENTS_PATH = '/gateway/v1/events';
const AUTH_TIMEOUT_MS = 5_000;
const HEARTBEAT_INTERVAL_MS = 30_000;

type Session = {
  socket: WebSocket;
  rawToken?: string;
  token?: DeveloperToken;
  alive: boolean;
  authenticating: boolean;
  delivery: Promise<void>;
  authTimer: NodeJS.Timeout;
};

function bearer(value: string | string[] | undefined) {
  const header = Array.isArray(value) ? value[0] : value;
  return header?.replace(/^Bearer\s+/i, '');
}

function send(socket: WebSocket, value: unknown) {
  if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify(value));
}

export function attachGatewayWebSocket(server: Server, _deprecatedOptions: Record<string, unknown> = {}) {
  const wss = new WebSocketServer({ noServer: true });
  const sessions = new Set<Session>();

  const authenticate = async (session: Session, rawToken: string | undefined) => {
    if (session.token || session.authenticating) return;
    session.authenticating = true;
    const token = await authenticateToken(rawToken, 'messages:read').catch(() => null);
    session.authenticating = false;
    if (!token || !token.ownerId || token.ownerId === '__legacy__') {
      send(session.socket, { type: 'error', error: { code: 'UNAUTHORIZED', message: 'Token 无效、已过期或缺少 messages:read 权限' } });
      session.socket.close(1008, 'Unauthorized');
      return;
    }
    clearTimeout(session.authTimer);
    session.rawToken = rawToken;
    session.token = token;
    send(session.socket, { type: 'connected', occurredAt: new Date().toISOString() });
  };

  wss.on('connection', (socket, request) => {
    const session: Session = {
      socket,
      alive: true,
      authenticating: false,
      delivery: Promise.resolve(),
      authTimer: setTimeout(() => socket.close(1008, 'Authentication timeout'), AUTH_TIMEOUT_MS),
    };
    sessions.add(session);
    socket.on('pong', () => { session.alive = true; });
    socket.on('message', (raw) => {
      if (session.token) return;
      try {
        const message = JSON.parse(raw.toString()) as { type?: unknown; token?: unknown };
        if (message.type !== 'authenticate' || typeof message.token !== 'string') throw new Error();
        void authenticate(session, message.token);
      } catch {
        send(socket, { type: 'error', error: { code: 'INVALID_MESSAGE', message: '首条消息必须是 authenticate 请求' } });
        socket.close(1008, 'Invalid authentication message');
      }
    });
    socket.on('close', () => { clearTimeout(session.authTimer); sessions.delete(session); });
    socket.on('error', () => undefined);
    const headerToken = bearer(request.headers.authorization);
    if (headerToken) void authenticate(session, headerToken);
  });

  const onUpgrade = (request: IncomingMessage, socket: Duplex, head: Buffer) => {
    let pathname: string;
    try { pathname = new URL(request.url ?? '', 'http://localhost').pathname; }
    catch { socket.destroy(); return; }
    if (pathname !== EVENTS_PATH) {
      socket.write('HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n');
      socket.destroy();
      return;
    }
    wss.handleUpgrade(request, socket, head, (websocket) => wss.emit('connection', websocket, request));
  };
  server.on('upgrade', onUpgrade);

  const deliver = (event: GatewayMessageCreatedEvent) => {
    for (const session of sessions) {
      if (!session.token?.accountIds.includes(event.accountId) || !session.rawToken) continue;
      session.delivery = session.delivery.then(async () => {
        const active = await authenticateToken(session.rawToken, 'messages:read');
        if (!active || !active.ownerId || active.ownerId === '__legacy__') { session.socket.close(1008, 'Token expired or revoked'); return; }
        send(session.socket, { id: event.id, type: event.type, occurredAt: event.occurredAt, data: event.data });
      }).catch(() => session.socket.close(1011, 'Event delivery failed'));
    }
  };
  const unsubscribe = gatewayEvents.subscribe(deliver);
  let eventCursor = getSyncStore().latestEventId();
  const eventTimer = setInterval(() => {
    for (const event of getSyncStore().listEvents(eventCursor)) {
      eventCursor = event.id;
      if (event.type !== 'message.created') continue;
      deliver({ id: String(event.id), type: 'message.created', occurredAt: event.createdAt, accountId: event.accountId, data: event.payload as GatewayMessageCreatedEvent['data'] });
    }
  }, 1_000);
  eventTimer.unref();

  const heartbeatTimer = setInterval(() => {
    for (const session of sessions) {
      if (!session.alive) { session.socket.terminate(); continue; }
      session.alive = false;
      session.socket.ping();
    }
  }, HEARTBEAT_INTERVAL_MS);
  heartbeatTimer.unref();

  const close = () => {
    clearInterval(heartbeatTimer);
    clearInterval(eventTimer);
    unsubscribe();
    server.off('upgrade', onUpgrade);
    for (const session of sessions) session.socket.terminate();
    wss.close();
  };
  server.once('close', close);
  return { close, path: EVENTS_PATH };
}
