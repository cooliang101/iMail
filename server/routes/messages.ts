import { Router } from 'express';
import { z } from 'zod';
import { buildNotifications } from '../domain/notifications.js';
import { notFound } from '../domain/errors.js';
import { asyncRoute } from '../http/async-route.js';
import { contactLogoKey, contactRootLogoKey, contactsNeedLogoUpdate } from '../contact-model.js';
import { mailboxRoleSchema, sendSchema } from '../http/schemas.js';
import { downloadAttachment, moveRemoteMessage, sendMessage, updateRemoteMessageFlags } from '../mail.js';
import { senderLogo } from '../sender-logo.js';
import { getCachedMessage, getMessageStats, listCachedMessages, readStore, updateStore } from '../store.js';

export const messagesRouter = Router();

function logoUrl(address: string) { return `/api/contacts/logo?address=${encodeURIComponent(address)}`; }

function contactView<T extends { address: string; logo?: object }>(contact: T) {
  return { ...contact, logo: { ...contact.logo, url: logoUrl(contact.address) } };
}

async function rememberLogo(
  address: string,
  logo: Awaited<ReturnType<typeof senderLogo>>,
  currentContacts?: Awaited<ReturnType<typeof readStore>>['contacts'],
) {
  if (!logo) return;
  const exactKey = contactLogoKey(address);
  const rootKey = contactRootLogoKey(address);
  if (!exactKey || !rootKey) return;
  const storedContacts = currentContacts ?? (await readStore()).contacts ?? [];
  if (!contactsNeedLogoUpdate(storedContacts, address, logo)) return;
  await updateStore((data) => {
    for (const contact of data.contacts ?? []) {
      const contactExactKey = contactLogoKey(contact.address);
      const contactRootKey = contactRootLogoKey(contact.address);
      if (logo.key === rootKey ? contactRootKey !== rootKey : contactExactKey !== exactKey) continue;
      contact.logo = { key: logo.key, contentType: logo.contentType, sourceUrl: logo.sourceUrl, fetchedAt: logo.fetchedAt };
    }
  });
}

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
  const data = await readStore();
  const contacts = new Map((data.contacts ?? []).map((contact) => [contact.address.toLocaleLowerCase(), contact]));
  const messages = result.messages.map(({ text: _text, html: _html, ...summary }) => ({
    ...summary, from: contactView({ ...summary.from, logo: contacts.get(summary.from.address.toLocaleLowerCase())?.logo }),
  }));
  res.json({ messages, total: result.total, nextOffset: input.offset + messages.length, hasMore: input.offset + messages.length < result.total });
}));

messagesRouter.get('/message-stats', asyncRoute(async (_req, res) => res.json(await getMessageStats())));

messagesRouter.get('/contacts', asyncRoute(async (_req, res) => {
  const data = await readStore();
  res.json({ contacts: (data.contacts ?? []).map(contactView) });
}));

messagesRouter.get('/contacts/logo', asyncRoute(async (req, res) => {
  const { address } = z.object({ address: z.string().trim().min(3).max(320) }).parse(req.query);
  const data = await readStore();
  const normalized = address.toLocaleLowerCase();
  if (!(data.contacts ?? []).some((contact) => contact.address.toLocaleLowerCase() === normalized)) { res.status(404).end(); return; }
  const message = data.messages
    .filter((item) => item.from.address.trim().toLocaleLowerCase() === normalized)
    .sort((left, right) => right.date.localeCompare(left.date))[0];
  const logo = await senderLogo(message ?? { from: { name: '', address }, text: '', html: '' });
  if (!logo) { res.setHeader('Cache-Control', 'private, max-age=3600'); res.status(404).end(); return; }
  await rememberLogo(address, logo, data.contacts ?? []);
  res.setHeader('Content-Type', logo.contentType);
  res.setHeader('Content-Length', String(logo.content.length));
  res.setHeader('Cache-Control', 'private, max-age=86400');
  res.setHeader('X-Content-Type-Options', 'nosniff');
  res.send(logo.content);
}));

messagesRouter.get('/messages/:id', asyncRoute(async (req, res) => {
  const message = await getCachedMessage(String(req.params.id));
  if (!message) { res.status(404).json({ error: '邮件不存在' }); return; }
  const data = await readStore();
  const logo = (data.contacts ?? []).find((contact) => contact.address.toLocaleLowerCase() === message.from.address.toLocaleLowerCase())?.logo;
  res.json({ message: { ...message, from: contactView({ ...message.from, logo }) } });
}));

messagesRouter.get('/messages/:id/sender-logo', asyncRoute(async (req, res) => {
  const message = await getCachedMessage(String(req.params.id));
  if (!message) { res.status(404).end(); return; }
  const logo = await senderLogo(message);
  if (!logo) { res.setHeader('Cache-Control', 'private, max-age=3600'); res.status(404).end(); return; }
  await rememberLogo(message.from.address, logo);
  res.setHeader('Content-Type', logo.contentType);
  res.setHeader('Content-Length', String(logo.content.length));
  res.setHeader('Cache-Control', 'private, max-age=86400');
  res.setHeader('X-Content-Type-Options', 'nosniff');
  res.send(logo.content);
}));

messagesRouter.patch('/messages/:id', asyncRoute(async (req, res) => {
  const input = z.object({ unread: z.boolean().optional(), flagged: z.boolean().optional(), labels: z.array(z.string().trim().min(1).max(40)).max(12).optional(), snoozedUntil: z.string().datetime().nullable().optional() }).parse(req.body);
  if (input.unread !== undefined || input.flagged !== undefined) await updateRemoteMessageFlags(String(req.params.id), { unread: input.unread, flagged: input.flagged });
  let updated;
  await updateStore((data) => {
    updated = data.messages.find((item) => item.id === req.params.id);
    if (!updated) throw notFound('MESSAGE_NOT_FOUND', '邮件不存在');
    Object.assign(updated, input, { snoozedUntil: input.snoozedUntil ?? undefined });
  });
  const data = await readStore();
  const logo = (data.contacts ?? []).find((contact) => contact.address.toLocaleLowerCase() === updated!.from.address.toLocaleLowerCase())?.logo;
  res.json({ message: { ...updated!, from: contactView({ ...updated!.from, logo }) } });
}));

messagesRouter.post('/messages/:id/move', asyncRoute(async (req, res) => {
  const { destination } = z.object({ destination: z.enum(['archive', 'trash']) }).parse(req.body);
  const result = await moveRemoteMessage(String(req.params.id), destination);
  let moved;
  await updateStore((data) => {
    moved = data.messages.find((item) => item.id === req.params.id);
    if (!moved) throw notFound('MESSAGE_NOT_FOUND', '邮件不存在');
    moved.mailbox = result.mailbox; moved.mailboxRole = destination;
    if (result.uid) moved.uid = result.uid;
    moved.snoozedUntil = undefined;
  });
  const data = await readStore();
  const logo = (data.contacts ?? []).find((contact) => contact.address.toLocaleLowerCase() === moved!.from.address.toLocaleLowerCase())?.logo;
  res.json({ message: { ...moved!, from: contactView({ ...moved!.from, logo }) }, destination, mailbox: result.mailbox });
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
  res.json({ notifications: buildNotifications(await readStore()) });
}));

messagesRouter.post('/send', asyncRoute(async (req, res) => {
  const input = sendSchema.parse(req.body);
  const result = await sendMessage(input);
  if (input.draftId) await updateStore((data) => { data.drafts = (data.drafts ?? []).filter((item) => item.id !== input.draftId); });
  res.status(201).json(result);
}));
