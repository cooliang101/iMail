import { lazy, Suspense, useEffect, useState } from 'preact/compat';
import type { Account, Message } from '../../types';
import type { ComposeMode, MessageBodyView } from '../../app-model';
import { api } from '../../services';
import { formatDate } from '../../components/date-format';
import { AppButton } from '../../components/AppButton';
import { MessageBody } from './MessageBody';
import '../../styles/conversation.css';

const AttachmentList = lazy(() => import('../attachments/AttachmentList').then(module => ({ default: module.AttachmentList })));

export function ConversationPanel({ messageId, accounts, view, onCompose }: {
  messageId: string; accounts: Account[]; view: MessageBodyView; onCompose: (message: Message, mode: ComposeMode) => void;
}) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setMessages([]); setError('');
    api<{ messages: Message[] }>(`/api/messages/${encodeURIComponent(messageId)}/conversation`, { signal: controller.signal })
      .then(result => { if (!controller.signal.aborted) setMessages(result.messages); })
      .catch(cause => { if (!controller.signal.aborted) setError(cause instanceof Error ? cause.message : '会话加载失败'); });
    return () => controller.abort();
  }, [messageId, revision]);
  if (error) return <section className="mail-conversation"><p role="alert">{error}</p><AppButton onClick={() => setRevision(value => value + 1)}>重试加载会话</AppButton></section>;
  if (messages.length <= 1) return null;
  return <section className="mail-conversation" aria-label="邮件会话">
    <header><h3>往来会话 · {messages.length} 封</h3><AppButton appearance="subtle" onClick={() => setRevision(value => value + 1)}>刷新会话</AppButton></header>
    <p>本地缓存中的往来历史。展开不会标记已读；邮件操作仅作用于当前打开的邮件。</p>
    {messages.map(message => message.id === messageId
      ? <div className="conversation-current" key={message.id}><strong>当前邮件</strong><span>{accounts.find(account => account.id === message.accountId)?.email} · {message.mailbox}</span></div>
      : <ConversationMessage key={message.id} summary={message} account={accounts.find(account => account.id === message.accountId)} view={view} onCompose={onCompose} />)}
  </section>;
}

function ConversationMessage({ summary, account, view, onCompose }: {
  summary: Message; account?: Account; view: MessageBodyView; onCompose: (message: Message, mode: ComposeMode) => void;
}) {
  const [open, setOpen] = useState(false);
  const [detail, setDetail] = useState<Message>();
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    if (!open || detail) return;
    const controller = new AbortController();
    setError('');
    api<{ message: Message }>(`/api/messages/${encodeURIComponent(summary.id)}`, { signal: controller.signal })
      .then(result => { if (!controller.signal.aborted) setDetail(result.message); })
      .catch(cause => { if (!controller.signal.aborted) setError(cause instanceof Error ? cause.message : '邮件加载失败'); });
    return () => controller.abort();
  }, [open, detail, summary.id, revision]);
  return <details className="conversation-message" open={open} onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>
      <strong>{summary.from.name || summary.from.address}</strong><span>{summary.subject}</span>
      <small>{account?.email ?? summary.accountId} · {summary.mailbox} · {formatDate(summary.date, { dateStyle: 'short', timeStyle: 'medium' }, 'zh-CN', '时间未知')} {summary.unread ? '· 未读' : ''}</small>
    </summary>
    {open && <div className="conversation-message-content">
      {error ? <><p role="alert">{error}</p><AppButton onClick={() => setRevision(value => value + 1)}>重试</AppButton></> : detail ? <>
        <div><AppButton onClick={() => onCompose(detail, 'reply')}>回复此邮件</AppButton><AppButton onClick={() => onCompose(detail, 'replyAll')}>回复全部</AppButton><AppButton onClick={() => onCompose(detail, 'forward')}>转发此邮件</AppButton></div>
        <MessageBody text={detail.text ?? ''} html={detail.html} subject={detail.subject} view={view} />
        {!!detail.attachments.length && <Suspense fallback={<p>正在加载附件…</p>}><AttachmentList messageId={detail.id} attachments={detail.attachments} /></Suspense>}
      </> : <p role="status">正在加载邮件…</p>}
    </div>}
  </details>;
}
