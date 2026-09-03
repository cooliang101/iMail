import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'preact-render-to-string';
import { parseHTML } from 'linkedom';
import type { Account, Message } from '../../types';
import { MessageReader } from './MessageReader';
import { PlatformProvider } from '../../platform/runtime';
import type { PlatformRuntime } from '../../platform/types';

const runtime: PlatformRuntime = {
  kind: 'web', openExternal: vi.fn(), saveDownload: vi.fn(), saveImage: vi.fn(), saveText: vi.fn(), prepareNotifications: vi.fn(async () => false),
  notify: vi.fn(), subscribeNotificationClicks: vi.fn(() => () => undefined),
};

const account: Account = {
  id: 'account-1', provider: 'custom', email: 'owner@example.test', displayName: 'Owner',
  group: 'Personal', groupIcon: 'folder', color: '#168f78', status: 'connected', mailboxes: [],
};
const message: Message = {
  id: 'message-1', accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox',
  from: { name: 'Sender', address: 'sender@example.test', logo: { url: '' } },
  to: [{ name: 'Owner', address: account.email }], subject: 'Hello', preview: 'Hello',
  text: 'Hello', date: '2026-08-31T00:00:00Z', unread: false, flagged: false,
  hasAttachments: false, attachments: [], labels: [],
};

function renderReader(currentMessage: Message) {
  return parseHTML(renderToStaticMarkup(<PlatformProvider runtime={runtime}><MessageReader
    message={currentMessage} account={account} accounts={[account]} contacts={[]}
    defaultBodyView="source" onComposeConversationMessage={vi.fn()}
    onReply={vi.fn()} onReplyAll={vi.fn()} onForward={vi.fn()} onComposeSender={vi.fn()}
    onFilterParticipant={vi.fn()} onCloseMobile={vi.fn()} onToggleFlag={vi.fn()}
    onArchive={vi.fn()} onDelete={vi.fn()} onSnooze={vi.fn()} onAddToWorkQueue={vi.fn()} onManageLabels={vi.fn()}
    onMarkUnread={vi.fn()} onPrevious={vi.fn()} onNext={vi.fn()}
    hasPrevious={false} hasNext={false} actionBusy={false}
  /></PlatformProvider>)).document;
}

describe('MessageReader reply-all toolbar action', () => {
  it('uses a distinct icon without overflowing text and preserves its accessible name', () => {
    const document = renderReader(message);
    const replyAll = document.querySelector('.sender-actions > button[aria-label="回复全部"]')!;
    const reply = document.querySelector('.sender-actions > button[aria-label="回复邮件"]')!;
    expect(replyAll.getAttribute('title')).toBe('回复全部');
    expect(replyAll.textContent).toBe('');
    expect(replyAll.children).toHaveLength(1);
    expect(replyAll.querySelector('svg')?.getAttribute('width')).toBe('18');
    expect(replyAll.querySelector('svg')?.innerHTML).not.toBe(reply.querySelector('svg')?.innerHTML);
    expect(replyAll.hasAttribute('disabled')).toBe(false);
  });

  it('keeps reply-all disabled until the message body has loaded', () => {
    const document = renderReader({ ...message, text: undefined });
    expect(document.querySelector('.sender-actions > button[aria-label="回复全部"]')?.hasAttribute('disabled')).toBe(true);
  });
});
