import { describe, expect, it } from 'vitest';
import { reconcileContacts } from './contact-model.js';
import type { CachedMessage, MailAccount } from './types.js';

const account: MailAccount = {
  id: 'account-1', provider: 'gmail', email: 'owner@example.com', displayName: 'Owner', group: '个人', color: '#168f78',
  settings: { imapHost: 'imap.example.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.example.com', smtpPort: 465, smtpSecure: true },
  encryptedSecret: 'encrypted', createdAt: '2026-08-01T00:00:00.000Z', status: 'connected',
};

function message(input: Pick<CachedMessage, 'id' | 'from' | 'to' | 'date'>): CachedMessage {
  return {
    ...input, accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox', uid: Number(input.id), subject: 'Subject', preview: '', text: '',
    unread: false, flagged: false, hasAttachments: false, attachments: [],
  };
}

describe('reconcileContacts', () => {
  it('collects both senders and recipients while excluding the signed-in mailbox', () => {
    const contacts = reconcileContacts({
      accounts: [account], contacts: [], messages: [
        message({ id: '1', date: '2026-08-05T01:00:00.000Z', from: { name: 'Alice', address: 'alice@example.net' }, to: [{ name: 'Owner', address: 'owner@example.com' }] }),
        message({ id: '2', date: '2026-08-05T02:00:00.000Z', from: { name: 'Owner', address: 'OWNER@example.com' }, to: [{ name: 'Bob', address: 'bob@example.net' }, { name: 'Alice', address: 'alice@example.net' }] }),
      ],
    });

    expect(contacts.map(({ address, messageCount }) => ({ address, messageCount }))).toEqual([
      { address: 'alice@example.net', messageCount: 2 },
      { address: 'bob@example.net', messageCount: 1 },
    ]);
  });
});
