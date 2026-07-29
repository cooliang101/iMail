import { simpleParser } from 'mailparser';
import { readStore } from '../store.js';
import { describeProtocolError, imapClientFor } from './client.js';

function sourceMessage(store: Awaited<ReturnType<typeof readStore>>, messageId: string) {
  const message = store.messages.find((item) => item.id === messageId);
  if (!message) throw new Error('邮件不存在');
  const account = store.accounts.find((item) => item.id === message.accountId);
  if (!account) throw new Error('邮箱账户不存在');
  return { message, account };
}

export async function downloadAttachment(messageId: string, attachmentIndex: number): Promise<{ content: Buffer; filename: string; contentType: string }> {
  const { message, account } = sourceMessage(await readStore(), messageId);
  const metadata = message.attachments[attachmentIndex];
  if (!metadata) throw new Error('附件不存在');
  const client = await imapClientFor(account);
  try {
    await client.connect();
    await client.mailboxOpen(message.mailbox, { readOnly: true });
    for await (const item of client.fetch(message.uid, { uid: true, source: true }, { uid: true })) {
      if (!item.source) continue;
      const parsed = await simpleParser(item.source);
      const attachment = parsed.attachments[attachmentIndex];
      if (!attachment) break;
      return { content: Buffer.from(attachment.content), filename: attachment.filename ?? metadata.filename, contentType: attachment.contentType || metadata.contentType };
    }
    throw new Error('无法从邮箱服务器读取附件');
  } catch (error) {
    if (error instanceof Error && (error.message === '附件不存在' || error.message.startsWith('无法从'))) throw error;
    throw describeProtocolError('IMAP', error);
  } finally { await client.logout().catch(() => undefined); }
}

export async function updateRemoteMessageFlags(messageId: string, input: { unread?: boolean; flagged?: boolean }): Promise<void> {
  const { message, account } = sourceMessage(await readStore(), messageId);
  const client = await imapClientFor(account);
  try {
    await client.connect();
    await client.mailboxOpen(message.mailbox, { readOnly: false });
    if (input.unread !== undefined) {
      if (input.unread) await client.messageFlagsRemove(message.uid, ['\\Seen'], { uid: true });
      else await client.messageFlagsAdd(message.uid, ['\\Seen'], { uid: true });
    }
    if (input.flagged !== undefined) {
      if (input.flagged) await client.messageFlagsAdd(message.uid, ['\\Flagged'], { uid: true });
      else await client.messageFlagsRemove(message.uid, ['\\Flagged'], { uid: true });
    }
  } catch (error) { throw describeProtocolError('IMAP', error); }
  finally { await client.logout().catch(() => undefined); }
}

export async function moveRemoteMessage(messageId: string, destination: 'archive' | 'trash'): Promise<{ mailbox: string; uid?: number }> {
  const { message, account } = sourceMessage(await readStore(), messageId);
  const client = await imapClientFor(account);
  try {
    await client.connect();
    const mailboxes = await client.list();
    const specialUses = destination === 'archive' ? ['\\Archive', '\\All'] : ['\\Trash'];
    const target = mailboxes.find((mailbox) => specialUses.includes(mailbox.specialUse ?? ''));
    if (!target) throw new Error(`服务商没有返回${destination === 'archive' ? '归档' : '垃圾箱'}文件夹`);
    await client.mailboxOpen(message.mailbox, { readOnly: false });
    const moved = await client.messageMove(message.uid, target.path, { uid: true });
    if (!moved) throw new Error('服务商未确认邮件移动操作');
    const uid = (moved as { uidMap?: Map<number, number> }).uidMap?.get(message.uid);
    return { mailbox: target.path, ...(uid ? { uid } : {}) };
  } catch (error) { throw describeProtocolError('IMAP', error); }
  finally { await client.logout().catch(() => undefined); }
}
