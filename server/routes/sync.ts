import { Router } from 'express';
import { z } from 'zod';
import { asyncRoute } from '../http/async-route.js';
import { readStore } from '../store.js';
import { getSyncStore } from '../sync/store.js';

const folderModeSchema = z.enum(['inbox', 'standard', 'selected']);
const policyChangesSchema = z.object({
  enabled: z.boolean().optional(),
  intervalMinutes: z.number().int().min(1).max(60).optional(),
  folderMode: folderModeSchema.optional(),
  selectedMailboxes: z.array(z.string().trim().min(1).max(500)).max(100).optional(),
  syncOnStart: z.boolean().optional(),
  retryOnRecovery: z.boolean().optional(),
  notifyOnError: z.boolean().optional(),
}).refine((value) => Object.keys(value).length > 0, '至少提供一个同步设置');

export const syncRouter = Router();

syncRouter.get('/sync-policy', asyncRoute(async (_req, res) => {
  res.json({ policy: getSyncStore().getDefaultPolicy() });
}));

syncRouter.patch('/sync-policy', asyncRoute(async (req, res) => {
  res.json({ policy: getSyncStore().updateDefaultPolicy(policyChangesSchema.parse(req.body)) });
}));

syncRouter.get('/accounts/:id/sync-policy', asyncRoute(async (req, res) => {
  const { accounts } = await readStore();
  if (!accounts.some((account) => account.id === req.params.id)) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  res.json({ policy: getSyncStore().ensurePolicy(String(req.params.id)) });
}));

syncRouter.patch('/accounts/:id/sync-policy', asyncRoute(async (req, res) => {
  const { accounts } = await readStore();
  const account = accounts.find((item) => item.id === req.params.id);
  if (!account) { res.status(404).json({ error: '邮箱账户不存在' }); return; }
  const changes = policyChangesSchema.parse(req.body);
  if (changes.selectedMailboxes) {
    const selectable = new Set((account.mailboxes ?? []).filter((item) => item.selectable).map((item) => item.path));
    if (changes.selectedMailboxes.some((mailbox) => !selectable.has(mailbox))) { res.status(400).json({ error: '同步文件夹必须来自该账户已发现的可选择文件夹' }); return; }
  }
  res.json({ policy: getSyncStore().updatePolicy(account.id, changes) });
}));

syncRouter.get('/sync-status', asyncRoute(async (_req, res) => {
  const { accounts } = await readStore();
  const syncStore = getSyncStore();
  const result = accounts.map((account) => {
    const policy = syncStore.ensurePolicy(account.id);
    const states = syncStore.listMailboxStates(account.id);
    const jobs = syncStore.listJobs({ accountId: account.id, limit: 10 });
    return { accountId: account.id, policy, states, jobs };
  });
  res.json({ accounts: result, worker: syncStore.workerHealth() });
}));

syncRouter.get('/sync-jobs/:id', asyncRoute(async (req, res) => {
  const job = getSyncStore().getJob(String(req.params.id));
  if (!job) { res.status(404).json({ error: '同步任务不存在' }); return; }
  res.json({ job });
}));

syncRouter.get('/events', (req, res) => {
  res.status(200);
  res.setHeader('Content-Type', 'text/event-stream; charset=utf-8');
  res.setHeader('Cache-Control', 'no-cache, no-transform');
  res.setHeader('Connection', 'keep-alive');
  res.flushHeaders();
  const requestedCursor = req.headers['last-event-id'] ?? req.query.after;
  let cursor = requestedCursor === undefined ? getSyncStore().latestEventId() : Number(requestedCursor);
  if (!Number.isFinite(cursor) || cursor < 0) cursor = 0;
  const send = () => {
    for (const event of getSyncStore().listEvents(cursor)) {
      cursor = event.id;
      res.write(`id: ${event.id}\nevent: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
    }
  };
  res.write(`event: connected\ndata: ${JSON.stringify({ connectedAt: new Date().toISOString() })}\n\n`);
  send();
  const eventTimer = setInterval(send, 1_000);
  const heartbeatTimer = setInterval(() => res.write(`: heartbeat ${Date.now()}\n\n`), 15_000);
  req.once('close', () => { clearInterval(eventTimer); clearInterval(heartbeatTimer); });
});
