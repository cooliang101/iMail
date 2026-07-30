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

function syncStatusSnapshot(accountIds: Iterable<string>) {
  const syncStore = getSyncStore();
  const accounts = Array.from(accountIds, (accountId) => ({
    accountId,
    policy: syncStore.ensurePolicy(accountId),
    states: syncStore.listMailboxStates(accountId),
    jobs: syncStore.listJobs({ accountId, limit: 10 }),
  }));
  return { accounts, worker: syncStore.workerHealth() };
}

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
  res.json(syncStatusSnapshot(accounts.map((account) => account.id)));
}));

syncRouter.get('/sync-jobs/:id', asyncRoute(async (req, res) => {
  const job = getSyncStore().getJob(String(req.params.id));
  const accountIds = new Set((await readStore()).accounts.map((account) => account.id));
  if (!job || !accountIds.has(job.accountId)) { res.status(404).json({ error: '同步任务不存在' }); return; }
  res.json({ job });
}));

syncRouter.get('/events', asyncRoute(async (req, res) => {
  const accountIds = new Set((await readStore()).accounts.map((account) => account.id));
  res.status(200);
  res.setHeader('Content-Type', 'text/event-stream; charset=utf-8');
  res.setHeader('Cache-Control', 'no-cache, no-transform');
  res.setHeader('Connection', 'keep-alive');
  res.flushHeaders();
  const requestedCursor = req.headers['last-event-id'] ?? req.query.after;
  let cursor = requestedCursor === undefined ? getSyncStore().latestEventId() : Number(requestedCursor);
  if (!Number.isFinite(cursor) || cursor < 0) cursor = 0;
  const sendStatus = () => {
    res.write(`event: sync.status\ndata: ${JSON.stringify(syncStatusSnapshot(accountIds))}\n\n`);
  };
  const send = () => {
    let changed = false;
    for (const event of getSyncStore().listEvents(cursor)) {
      cursor = event.id;
      if (!accountIds.has(event.accountId)) continue;
      res.write(`id: ${event.id}\nevent: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
      changed = true;
    }
    if (changed) sendStatus();
  };
  res.write(`event: connected\ndata: ${JSON.stringify({ connectedAt: new Date().toISOString() })}\n\n`);
  send();
  sendStatus();
  const eventTimer = setInterval(send, 1_000);
  const heartbeatTimer = setInterval(sendStatus, 15_000);
  req.once('close', () => { clearInterval(eventTimer); clearInterval(heartbeatTimer); });
}));
