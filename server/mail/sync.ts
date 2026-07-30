import { createHash } from 'node:crypto';
import type { FetchMessageObject, ListResponse } from 'imapflow';
import { simpleParser } from 'mailparser';
import { commitMailboxSync, readStore, setAccountSyncStatus } from '../store.js';
import type { CachedMessage, MailboxFolder, MailboxMessageChange, MailboxRole } from '../types.js';
import { gatewayEvents } from '../gateway/events.js';
import { address, imapClientFor } from './client.js';
import { mailboxRoleFor } from './mailbox-role.js';

const specialUseForRole: Partial<Record<MailboxRole, string[]>> = {
  sent: ['\\Sent'], archive: ['\\Archive', '\\All'], drafts: ['\\Drafts'], trash: ['\\Trash'], junk: ['\\Junk'],
};

function publicMailbox(item: Partial<ListResponse> & { path: string }): MailboxFolder {
  return {
    path: item.path,
    name: item.name || item.path.split(item.delimiter || '/').at(-1) || item.path,
    delimiter: item.delimiter || '/',
    specialUse: item.specialUse,
    selectable: !item.flags?.has('\\Noselect'),
    subscribed: item.subscribed ?? true,
    total: item.status?.messages,
    unread: item.status?.unseen,
  };
}

export type SyncCursor = { uidValidity?: string; lastSeenUid?: number; highestModseq?: string };
export type SyncExecutionResult = {
  synced: number;
  created: number;
  updated: number;
  deleted: number;
  mailbox: string;
  mailboxRole: MailboxRole;
  uidValidity?: string;
  highestModseq?: string;
  lastSeenUid: number;
  createdMessages: CachedMessage[];
  messageChanges: MailboxMessageChange[];
};

export async function syncMailbox(accountId: string, mailboxRole: MailboxRole = 'inbox', requestedMailbox?: string, cursor?: SyncCursor): Promise<SyncExecutionResult> {
  const store = await readStore();
  const account = store.accounts.find((item) => item.id === accountId);
  if (!account) throw new Error('邮箱账户不存在');
  const previouslySynced = Boolean(account.lastSyncAt);
  await setAccountSyncStatus(accountId, 'syncing');

  const client = await imapClientFor(account);
  try {
    await client.connect();
    const listed = await client.list({ statusQuery: { messages: true, unseen: true } });
    const folders = listed.map(publicMailbox);
    let mailboxPath = 'INBOX';
    let allMailArchive = false;
    if (requestedMailbox) {
      const target = listed.find((item) => item.path === requestedMailbox);
      if (!target || target.flags?.has('\\Noselect')) throw new Error('邮箱文件夹不存在或不可选择');
      mailboxPath = target.path;
      mailboxRole = mailboxRoleFor(target);
      allMailArchive = mailboxRole === 'archive' && target.specialUse === '\\All';
    } else if (mailboxRole !== 'inbox') {
      const specialUses = specialUseForRole[mailboxRole] ?? [];
      const target = listed.find((item) => specialUses.includes(item.specialUse ?? '') || mailboxRoleFor(item) === mailboxRole);
      const roleLabel: Partial<Record<MailboxRole, string>> = { sent: '已发送', archive: '归档', drafts: '草稿', trash: '已删除邮件', junk: '垃圾邮件' };
      if (!target) throw new Error(`服务商没有返回${roleLabel[mailboxRole] ?? mailboxRole}文件夹`);
      mailboxPath = target.path;
      allMailArchive = mailboxRole === 'archive' && target.specialUse === '\\All';
    }
    const mailbox = await client.mailboxOpen(mailboxPath, { readOnly: true });
    const cached = store.messages.filter((message) => message.accountId === accountId && message.mailbox === mailboxPath);
    const uidValidity = mailbox.uidValidity === undefined ? undefined : String(mailbox.uidValidity);
    const highestModseq = mailbox.highestModseq === undefined ? undefined : String(mailbox.highestModseq);
    const uidValidityChanged = Boolean(cursor?.uidValidity && uidValidity && cursor.uidValidity !== uidValidity);
    const maxCachedUid = cached.reduce((max, message) => Math.max(max, message.uid), 0);
    const incrementalUid = uidValidityChanged ? 0 : Math.max(maxCachedUid, cursor?.lastSeenUid ?? 0);
    const incoming: CachedMessage[] = [];
    const flagUpdates = new Map<number, { unread: boolean; flagged: boolean }>();
    const checkedCachedUids = new Set<number>();
    let lastSeenUid = incrementalUid;

    const parseIncoming = async (items: AsyncIterable<FetchMessageObject>) => {
      for await (const item of items) {
        lastSeenUid = Math.max(lastSeenUid, item.uid);
        if (!item.source) continue;
        const labels = (item as FetchMessageObject & { labels?: Set<string> }).labels;
        if (allMailArchive && labels && ['\\Inbox', '\\Sent', '\\Drafts', '\\Trash'].some((label) => labels.has(label))) continue;
        const parsed = await simpleParser(item.source);
        const fromValue = parsed.from?.value[0];
        const toValue = parsed.to && !Array.isArray(parsed.to) ? parsed.to.value : Array.isArray(parsed.to) ? parsed.to.flatMap((entry) => entry.value) : [];
        const text = parsed.text?.trim() ?? '';
        const html = typeof parsed.html === 'string' ? parsed.html : undefined;
        incoming.push({
          id: createHash('sha256').update(mailboxRole === 'inbox' ? `${accountId}:${item.uid}` : mailboxRole === 'custom' ? `${accountId}:${mailboxPath}:${item.uid}` : `${accountId}:${mailboxRole}:${item.uid}`).digest('hex').slice(0, 24),
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

    if (mailbox.exists > 0 && incrementalUid === 0) {
      const start = Math.max(1, mailbox.exists - 79);
      await parseIncoming(client.fetch(`${start}:*`, { uid: true, flags: true, labels: true, source: true, envelope: true, internalDate: true }));
    } else if (incrementalUid > 0 && mailbox.uidNext > incrementalUid + 1) {
      await parseIncoming(client.fetch(`${incrementalUid + 1}:*`, { uid: true, flags: true, labels: true, source: true, envelope: true, internalDate: true }, { uid: true }));
    }

    const cachedUids = uidValidityChanged ? [] : cached.map((message) => message.uid);
    if (cachedUids.length > 0) {
      let changedSince: bigint | undefined;
      if (cursor?.highestModseq && highestModseq) {
        try { changedSince = BigInt(cursor.highestModseq); } catch { changedSince = undefined; }
      }
      for (let index = 0; index < cachedUids.length; index += 500) {
        const batch = cachedUids.slice(index, index + 500);
        for await (const item of client.fetch(batch, { uid: true, flags: !changedSince }, { uid: true })) {
          checkedCachedUids.add(item.uid);
          if (!changedSince) flagUpdates.set(item.uid, { unread: !item.flags?.has('\\Seen'), flagged: Boolean(item.flags?.has('\\Flagged')) });
        }
      }
      if (changedSince) {
        const minimumUid = Math.min(...cachedUids);
        for await (const item of client.fetch(`${minimumUid}:*`, { uid: true, flags: true }, { uid: true, changedSince })) {
          flagUpdates.set(item.uid, { unread: !item.flags?.has('\\Seen'), flagged: Boolean(item.flags?.has('\\Flagged')) });
        }
      }
    }

    const removedUids = new Set(cachedUids.filter((uid) => !checkedCachedUids.has(uid)));
    const updatedCount = cached.reduce((count, message) => {
      const flags = flagUpdates.get(message.uid);
      return count + (flags && (message.unread !== flags.unread || message.flagged !== flags.flagged) ? 1 : 0);
    }, 0);
    const { createdMessages, messageChanges } = await commitMailboxSync({
      accountId, mailbox: mailboxPath, mailboxRole, incoming, removedUids: [...removedUids], uidValidityChanged,
      flagUpdates: [...flagUpdates].map(([uid, flags]) => ({ uid, ...flags })), folders, completedAt: new Date().toISOString(),
    });
    if (previouslySynced && createdMessages.length > 0) gatewayEvents.publishMessageCreated(account, createdMessages);
    return {
      synced: incoming.length, created: createdMessages.length, updated: updatedCount, deleted: uidValidityChanged ? cached.length : removedUids.size,
      mailbox: mailboxPath, mailboxRole, uidValidity, highestModseq, lastSeenUid,
      createdMessages: previouslySynced ? createdMessages : [], messageChanges,
    };
  } catch (error) {
    const message = error instanceof Error ? error.message : '同步失败';
    await setAccountSyncStatus(accountId, 'error', message);
    throw error;
  } finally { await client.logout().catch(() => undefined); }
}

export async function syncAccount(accountId: string, mailboxRole: MailboxRole = 'inbox', requestedMailbox?: string): Promise<{ synced: number }> {
  const result = await syncMailbox(accountId, mailboxRole, requestedMailbox);
  return { synced: result.synced };
}
