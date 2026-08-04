import { Router } from 'express';
import { readFileSync } from 'node:fs';
import { daemonShutdownAuthorized, serviceInfo } from '../service-info.js';

export function createServiceInfoRouter(webClient = false) {
  const router = Router();

  router.get('/system/info', (_req, res) => {
  res.setHeader('Cache-Control', 'no-store');
    res.json(serviceInfo(webClient));
});

  router.post('/system/shutdown', (req, res) => {
  const controlFile = process.env.IMAIL_DAEMON_CONTROL_FILE;
  if (!controlFile) { res.status(404).json({ error: '此服务不由桌面守护程序管理' }); return; }
  let expectedToken: string | undefined;
  try { expectedToken = readFileSync(controlFile, 'utf8'); }
  catch { res.status(503).json({ error: '守护控制信息不可用' }); return; }
  if (!daemonShutdownAuthorized(req.socket.remoteAddress, req.header('x-imail-daemon-token'), expectedToken)) {
    res.status(403).json({ error: '守护控制授权失败' }); return;
  }
  res.status(202).json({ stopping: true });
  setImmediate(() => process.kill(process.pid, 'SIGTERM'));
});

  return router;
}

export const serviceInfoRouter = createServiceInfoRouter();
