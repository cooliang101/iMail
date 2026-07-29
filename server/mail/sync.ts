import { createHash } from 'node:crypto';
import type { FetchMessageObject } from 'imapflow';
import { simpleParser } from 'mailparser';
import { readStore, updateStore } from '../store.js';
import type { CachedMessage, MailboxRole } from '../types.js';
import { address, imapClientFor } from './client.js';

const specialUseForRole: Partial<Record<MailboxRole, string[]>> = {
  sent: ['\\Sent'], archive: ['\\Archive', '\\All'], trash: ['\\Trash'],
};

export async function syncAccount(accountId: string, mailboxRole: MailboxRole = 'inbox'): Promise<{ synced: number }> {
  const store = await readStore();
  const account = store.accounts.find((item) => item.id === accountId);
  if (!account) throw new Error('邮箱账户不存在');
  await updateStore((data) => {
    const current = data.accounts.find((item) => item.id === accountId);
    if (current) { current.status = 'syncing'; current.lastError = undefined; }
  });

  const client = await imapClientFor(account);
  try {
    await client.connect();
    let mailboxPath = 'INBOX';
    let allMailArchive = false;
    if (mailboxRole !== 'inbox') {
      const mailboxes = await client.list();
      const specialUses = specialUseForRole[mailboxRole] ?? [];
      const target = mailboxes.find((item) => specialUses.includes(item.specialUse ?? ''));
      if (!target) throw new Error(`服务商没有返回${mailboxRole === 'sent' ? '已发送' : mailboxRole === 'archive' ? '归档' : '垃圾箱'}文件夹`);
      mailboxPath = target.path;
      allMailArchive = mailboxRole === 'archive' && target.specialUse === '\\All';
    }
    const mailbox = await client.mailboxOpen(mailboxPath, { readOnly: true });
    const cached = store.messages.filter((message) => message.accountId === accountId && message.mailbox === mailboxPath);
    const maxCachedUid = cached.reduce((max, message) => Math.max(max, message.uid), 0);
    const incoming: CachedMessage[] = [];
    const flagUpdates = new Map<number, { unread: boolean; flagged: boolean }>();

    const parseIncoming = async (items: AsyncIterable<FetchMessageObject>) => {
      for await (const item of items) {
        if (!item.source) continue;
        const labels = (item as FetchMessageObject & { labels?: Set<string> }).labels;
        if (allMailArchive && labels && ['\\Inbox', '\\Sent', '\\Drafts', '\\Trash'].some((label) => labels.has(label))) continue;
        const parsed = await simpleParser(item.source);
        const fromValue = parsed.from?.value[0];
        const toValue = parsed.to && !Array.isArray(parsed.to) ? parsed.to.value : Array.isArray(parsed.to) ? parsed.to.flatMap((entry) => entry.value) : [];
        const text = parsed.text?.trim() ?? '';
        const html = typeof parsed.html === 'string' ? parsed.html : undefined;
        incoming.push({
          id: createHash('sha256').update(mailboxRole === 'inbox' ? `${accountId}:${item.uid}` : `${accountId}:${mailboxRole}:${item.uid}`).digest('hex').slice(0, 24),
          accountId, mailbox: mailboxPath, mailboxRole, uid: item.uid, messageId: parsed.messageId,
          from: address(fromValue), to: toValue.map(address), subject: parsed.subject?.trim() || '（无主题）',
          preview: text.replace(/\s+/g, ' ').slice(0, 180), text, html,
          date: new Date(parsed.date ?? item.internalDate ?? Date.now()).toISOString(),
          unread: !item.flags?.has('\\Seen'), flagged: Boolean(item.flags?.has('\\Flagged')),
          hasAttachments: parsed.attachments.length > 0,
          attachments: parsed.attachments.map((attachment, index) => ({
            filename: attachment.filename ?? 'attachment', contentType: attachment.contentType, size: attachment.size, index,
          })),
          labels: [],
        });
      }
    };

    if (mailbox.exists > 0 && maxCachedUid === 0) {
      const start = Math.max(1, mailbox.exists - 79);
      await parseIncoming(client.fetch(`${start}:*`, { uid: true, flags: true, labels: true, source: true, envelope: true, internalDate: true }));
    } else if (maxCachedUid > 0 && mailbox.uidNext > maxCachedUid + 1) {
      await parseIncoming(client.fetch(`${maxCachedUid + 1}:*`, { uid: true, flags: true, labels: true, source: true, envelope: true, internalDate: true }, { uid: true }));
    }

    const recentCachedUids = cached.sort((a, b) => b.uid - a.uid).slice(0, 100).map((message) => message.uid);
    if (recentCachedUids.length > 0) {
      for await (const item of client.fetch(recentCachedUids, { uid: true, flags: true }, { uid: true })) {
        flagUpdates.set(item.uid, { unread: !item.flags?.has('\\Seen'), flagged: Boolean(item.flags?.has('\\Flagged')) });
      }
    }

    await updateStore((data) => {
      const ids = new Set(incoming.map((message) => message.id));
      for (const message of incoming) {
        const previous = data.messages.find((item) => item.id === message.id);
        if (previous) { message.labels = previous.labels ?? []; message.snoozedUntil = previous.snoozedUntil; }
      }
      const incomingMessageIds = new Set(incoming.map((message) => message.messageId).filter(Boolean));
      data.messages = [...data.messages.filter((message) => message.accountId !== accountId || (!ids.has(message.id) && !((message.mailboxRole ?? 'inbox') === mailboxRole && message.messageId && incomingMessageIds.has(message.messageId)))), ...incoming]
        .sort((a, b) => b.date.localeCompare(a.date)).slice(0, 5000);
      for (const message of data.messages) {
        if (message.accountId !== accountId || message.mailbox !== mailboxPath) continue;
        const flags = flagUpdates.get(message.uid);
        if (flags) Object.assign(message, flags);
      }
      const current = data.accounts.find((item) => item.id === accountId);
      if (current) { current.status = 'connected'; current.lastSyncAt = new Date().toISOString(); current.lastError = undefined; }
    });
    return { synced: incoming.length };
  } catch (error) {
    const message = error instanceof Error ? error.message : '同步失败';
    await updateStore((data) => {
      const current = data.accounts.find((item) => item.id === accountId);
      if (current) { current.status = 'error'; current.lastError = message; }
    });
    throw error;
  } finally { await client.logout().catch(() => undefined); }
}
