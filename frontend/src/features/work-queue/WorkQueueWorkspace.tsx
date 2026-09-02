import { useMemo, useState } from 'preact/hooks';
import { AppButton } from '../../components/AppButton';
import { AppSelect } from '../../components/form-controls';
import { CheckCircle, ClipboardText, Clock, PencilSimple } from '../../components/icons';
import { AccountProviderMark } from '../../components/provider-icons';
import type { Account, MailWorkItemView, MailWorkStatus } from '../../types';

const statusCopy: Record<MailWorkStatus, string> = {
  needsReply: '待回复', needsReview: '待确认', followUp: '待跟进', waiting: '等待对方',
};
const statusOptions = Object.entries(statusCopy).map(([value, label]) => ({ value, label }));

function dateLabel(value?: string) {
  if (!value) return '';
  return new Intl.DateTimeFormat('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(value));
}

export function WorkQueueWorkspace({ items, accounts, selectedId, onSelect, onUpdate, onComplete, onOpenDraft }: {
  items: MailWorkItemView[];
  accounts: Account[];
  selectedId?: string;
  onSelect: (messageId: string) => void;
  onUpdate: (messageId: string, status: MailWorkStatus) => void | Promise<void>;
  onComplete: (messageId: string) => void | Promise<void>;
  onOpenDraft: (draftId: string) => void;
}) {
  const [filter, setFilter] = useState<MailWorkStatus | 'all'>('all');
  const visible = useMemo(() => filter === 'all' ? items : items.filter(({ item }) => item.status === filter), [filter, items]);
  const overdue = items.filter(({ item }) => item.dueAt && new Date(item.dueAt).getTime() < Date.now()).length;
  return <section className="message-pane work-queue-pane">
    <header className="draft-pane-header work-queue-header"><div><strong>邮件处理队列</strong><small>{items.length} 项待处理{overdue ? `，${overdue} 项已到期` : ''}</small></div><AppSelect aria-label="筛选处理状态" value={filter} options={[{ value: 'all', label: '全部状态' }, ...statusOptions]} onValueChange={(value) => setFilter(value as MailWorkStatus | 'all')} /></header>
    {visible.length === 0 ? <div className="draft-empty"><ClipboardText size={42} weight="duotone" /><h2>当前没有处理项目</h2><p>在邮件阅读页加入队列，或让 Agent 按规则整理待办。</p></div> : <div className="work-queue-list app-scrollbar">
      {visible.map(({ item, message }) => {
        const account = accounts.find((value) => value.id === item.accountId);
        const isOverdue = item.dueAt && new Date(item.dueAt).getTime() < Date.now();
        return <article key={item.id} className={`work-queue-item ${selectedId === message.id ? 'is-selected' : ''}`} onClick={() => onSelect(message.id)}>
          <div className="work-queue-item-heading"><span>{account && <AccountProviderMark provider={account.provider} />}<span>{message.from.name || message.from.address}</span></span><em className={`work-queue-status is-${item.status}`}>{statusCopy[item.status]}</em></div>
          <strong>{message.subject || '（无主题）'}</strong><p>{message.preview}</p>
          {item.note && <small className="work-queue-note">{item.note}</small>}
          <footer onClick={(event) => event.stopPropagation()}><AppSelect aria-label={`更新 ${message.subject} 的处理状态`} value={item.status} options={statusOptions} onValueChange={(value) => void onUpdate(message.id, value as MailWorkStatus)} />{item.dueAt && <span className={isOverdue ? 'is-overdue' : ''}><Clock size={14} />{dateLabel(item.dueAt)}</span>}{item.draftId && <AppButton appearance="subtle" icon={<PencilSimple size={15} />} onClick={() => onOpenDraft(item.draftId!)}>打开草稿</AppButton>}<AppButton appearance="subtle" icon={<CheckCircle size={15} />} onClick={() => void onComplete(message.id)}>完成</AppButton></footer>
        </article>;
      })}
    </div>}
  </section>;
}
