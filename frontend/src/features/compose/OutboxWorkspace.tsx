import { AppButton } from '../../components/AppButton';
import { CheckCircle, Clock, PaperPlaneTilt, WarningCircle, X } from '../../components/icons';
import { AccountProviderMark } from '../../components/provider-icons';
import type { Account, OutboxItem } from '../../types';

const statusCopy: Record<OutboxItem['status'], string> = {
  scheduled: '等待发送', sending: '发送中', sent: '已发送', failed: '发送失败',
  needsReview: '待人工核对', cancelled: '已取消',
};

function scheduleLabel(value: string) {
  return new Intl.DateTimeFormat('zh-CN', {
    month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
  }).format(new Date(value));
}

export function OutboxWorkspace({ items, accounts, onCancel, onRetry }: {
  items: OutboxItem[];
  accounts: Account[];
  onCancel: (id: string) => void | Promise<void>;
  onRetry: (id: string) => void | Promise<void>;
}) {
  const pending = items.filter((item) => ['scheduled', 'sending', 'failed', 'needsReview'].includes(item.status));
  return <section className="message-pane outbox-pane">
    <header className="draft-pane-header"><div><span>服务端持久任务</span><strong>发件箱</strong><small>{pending.length} 封待处理 · 使用当前设备时区展示</small></div></header>
    {items.length === 0 ? <div className="draft-empty"><Clock size={40} weight="duotone" /><h2>没有待发送邮件</h2><p>写信时选择定时发送，任务会保存在当前 iMail 服务中。</p></div> : <div className="outbox-list app-scrollbar">
      {items.map((item) => {
        const account = accounts.find((value) => value.id === item.accountId);
        const StatusIcon = item.status === 'sent' ? CheckCircle : item.status === 'failed' || item.status === 'needsReview' ? WarningCircle : item.status === 'cancelled' ? X : item.status === 'sending' ? PaperPlaneTilt : Clock;
        return <article key={item.id} className={`outbox-item is-${item.status}`}>
          <div className="outbox-item-heading"><span>{account && <AccountProviderMark provider={account.provider} />}{account?.displayName ?? '未知邮箱'}</span><em className={`outbox-status is-${item.status}`}><StatusIcon size={14} />{statusCopy[item.status]}</em></div>
          <strong>{item.subject || '（无主题）'}</strong>
          <p>收件人：{item.to.join(', ') || item.cc.join(', ') || item.bcc.join(', ')}</p>
          <small>计划时间：{scheduleLabel(item.scheduledAt)}{item.attempts ? ` · 已尝试 ${item.attempts} 次` : ''}</small>
          {item.lastError && <div className="outbox-error"><WarningCircle size={14} /><span>{item.lastError}</span></div>}
          {(item.status === 'scheduled' || item.status === 'failed') && <footer>
            {item.status === 'failed' && <AppButton appearance="subtle" onClick={() => void onRetry(item.id)}>重新发送</AppButton>}
            <AppButton appearance="subtle" onClick={() => void onCancel(item.id)}>取消并返回草稿</AppButton>
          </footer>}
        </article>;
      })}
    </div>}
  </section>;
}

export function OutboxWelcome() {
  return <section className="composer-pane composer-welcome"><Clock size={48} weight="duotone" /><h2>定时发送由 iMail 服务执行</h2><p>服务离线期间任务不会发送；恢复运行后会处理到期任务。进入发送阶段后不能撤回。</p></section>;
}
