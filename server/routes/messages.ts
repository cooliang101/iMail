import { Router } from 'express';
import { z } from 'zod';
import { asyncRoute } from '../http/async-route.js';
import { mailboxRoleSchema, sendSchema } from '../http/schemas.js';
import { downloadAttachment, moveRemoteMessage, sendMessage, updateRemoteMessageFlags } from '../mail.js';
import { getCachedMessage, getMessageStats, listCachedMessages, readStore, updateStore } from '../store.js';

export const messagesRouter = Router();

messagesRouter.get('/messages', asyncRoute(async (req, res) => {
  const input = z.object({
    accountId: z.string().optional(), group: z.string().optional(), q: z.string().max(200).optional(),
    unread: z.enum(['true', 'false']).optional(), flagged: z.enum(['true', 'false']).optional(), hasAttachments: z.enum(['true', 'false']).optional(),
    mailboxRole: mailboxRoleSchema.optional(), mailbox: z.string().min(1).max(500).optional(), mailboxName: z.string().min(1).max(500).optional(), snoozed: z.enum(['true', 'false']).optional(), label: z.string().max(80).optional(),
    limit: z.coerce.number().int().min(1).max(100).default(60), offset: z.coerce.number().int().min(0).default(0),
  }).parse(req.query);
  const result = await listCachedMessages({
    accountId: input.accountId, group: input.group, query: input.q, mailbox: input.mailbox, mailboxName: input.mailboxName,
    unread: input.unread === 'true', flagged: input.flagged === 'true', hasAttachments: input.hasAttachments === 'true', mailboxRole: input.mailbox || input.mailboxName ? undefined : (input.mailboxRole ?? 'inbox'),
    snoozed: input.snoozed === 'true', label: input.label, limit: input.limit, offset: input.offset,
  });
  const messages = result.messages.map(({ text: _text, html: _html, ...summary }) => summary);
  res.json({ messages, total: result.total, nextOffset: input.offset + messages.length, hasMore: input.offset + messages.length < result.total });
}));

messagesRouter.get('/message-stats', asyncRoute(async (_req, res) => res.json(await getMessageStats())));

messagesRouter.get('/contacts', asyncRoute(async (_req, res) => {
  const data = await readStore();
  const ownAddresses = new Set(data.accounts.map((account) => account.email.trim().toLocaleLowerCase()));
  const contacts = new Map<string, { address: string; name: string; messageCount: number; lastContactAt: string }>();

  for (const message of data.messages) {
    const participants = [message.from, ...message.to];
    const seenInMessage = new Set<string>();
    for (const participant of participants) {
      const address = participant.address.trim();
      const key = address.toLocaleLowerCase();
      if (!address || ownAddresses.has(key) || seenInMessage.has(key)) continue;
      seenInMessage.add(key);
      const current = contacts.get(key);
      const isLatest = !current || message.date > current.lastContactAt;
      contacts.set(key, {
        address: isLatest ? address : current.address,
        name: isLatest ? (participant.name.trim() || current?.name || '') : current.name,
        messageCount: (current?.messageCount ?? 0) + 1,
        lastContactAt: current && current.lastContactAt > message.date ? current.lastContactAt : message.date,
      });
    }
  }

  res.json({ contacts: Array.from(contacts.values()).sort((left, right) => right.lastContactAt.localeCompare(left.lastContactAt) || right.messageCount - left.messageCount || left.address.localeCompare(right.address)) });
}));

messagesRouter.get('/messages/:id', asyncRoute(async (req, res) => {
  const message = await getCachedMessage(String(req.params.id));
  if (!message) { res.status(404).json({ error: '邮件不存在' }); return; }
  res.json({ message });
}));

messagesRouter.patch('/messages/:id', asyncRoute(async (req, res) => {
  const input = z.object({ unread: z.boolean().optional(), flagged: z.boolean().optional(), labels: z.array(z.string().trim().min(1).max(40)).max(12).optional(), snoozedUntil: z.string().datetime().nullable().optional() }).parse(req.body);
  if (input.unread !== undefined || input.flagged !== undefined) await updateRemoteMessageFlags(String(req.params.id), { unread: input.unread, flagged: input.flagged });
  let updated;
  await updateStore((data) => {
    updated = data.messages.find((item) => item.id === req.params.id);
    if (!updated) throw new Error('邮件不存在');
    Object.assign(updated, input, { snoozedUntil: input.snoozedUntil ?? undefined });
  });
  res.json({ message: updated });
}));

messagesRouter.post('/messages/:id/move', asyncRoute(async (req, res) => {
  const { destination } = z.object({ destination: z.enum(['archive', 'trash']) }).parse(req.body);
  const result = await moveRemoteMessage(String(req.params.id), destination);
  let moved;
  await updateStore((data) => {
    moved = data.messages.find((item) => item.id === req.params.id);
    if (!moved) throw new Error('邮件不存在');
    moved.mailbox = result.mailbox; moved.mailboxRole = destination;
    if (result.uid) moved.uid = result.uid;
    moved.snoozedUntil = undefined;
  });
  res.json({ message: moved, destination, mailbox: result.mailbox });
}));

messagesRouter.get('/messages/:id/attachments/:index', asyncRoute(async (req, res) => {
  const index = z.coerce.number().int().min(0).parse(req.params.index);
  const attachment = await downloadAttachment(String(req.params.id), index);
  const filename = attachment.filename.replace(/[\r\n"\\]/g, '_');
  res.setHeader('Content-Type', attachment.contentType || 'application/octet-stream');
  res.setHeader('Content-Disposition', `attachment; filename*=UTF-8''${encodeURIComponent(filename)}`);
  res.send(attachment.content);
}));

messagesRouter.get('/labels', asyncRoute(async (_req, res) => {
  const data = await readStore();
  const labels = Array.from(new Set(data.messages.flatMap((message) => message.labels ?? []))).sort((a, b) => a.localeCompare(b, 'zh-CN'));
  res.json({ labels });
}));

messagesRouter.get('/notifications', asyncRoute(async (_req, res) => {
  const data = await readStore(); const now = new Date().toISOString();
  const connection = data.accounts.filter((item) => item.status === 'error').map((account) => ({ id: `account-${account.id}`, kind: 'error', title: `${account.displayName} 连接异常`, detail: account.lastError || account.email, date: account.lastSyncAt || account.createdAt, accountId: account.id }));
  const returned = data.messages.filter((item) => item.snoozedUntil && item.snoozedUntil <= now).slice(0, 20).map((message) => ({ id: `snooze-${message.id}`, kind: 'snooze', title: message.subject, detail: '稍后处理的邮件已返回收件箱', date: message.snoozedUntil!, messageId: message.id, accountId: message.accountId }));
  const unread = data.messages.filter((item) => (item.mailboxRole ?? 'inbox') === 'inbox' && item.unread && (!item.snoozedUntil || item.snoozedUntil <= now)).slice(0, 20).map((message) => ({ id: `unread-${message.id}`, kind: 'unread', title: message.subject, detail: message.from.name || message.from.address, date: message.date, messageId: message.id, accountId: message.accountId }));
  res.json({ notifications: [...connection, ...returned, ...unread].sort((a, b) => b.date.localeCompare(a.date)).slice(0, 30) });
}));

messagesRouter.post('/send', asyncRoute(async (req, res) => {
  const input = sendSchema.parse(req.body);
  const result = await sendMessage(input);
  if (input.draftId) await updateStore((data) => { data.drafts = (data.drafts ?? []).filter((item) => item.id !== input.draftId); });
  res.status(201).json(result);
}));
