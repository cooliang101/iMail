import path from 'node:path';
import { createServer } from 'node:http';
import { pathToFileURL } from 'node:url';
import { app } from './app.js';
import { attachGatewayWebSocket } from './gateway/websocket.js';

export { app, createApp } from './app.js';

const port = Number(process.env.PORT ?? 8787);
const host = process.env.HOST ?? '127.0.0.1';

export function startServer() {
  const server = createServer(app);
  attachGatewayWebSocket(server);
  return server.listen(port, host, () => console.log(`iMail API running at http://${host}:${port}`));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) startServer();
