import { ArrowClockwise, CheckCircle, Clock, WarningCircle } from '@phosphor-icons/react';
import type { Account, AccountSyncStatus, MailboxRole, MailboxSyncState, SyncJob } from '../../types';

function newest<T>(values: T[], time: (value: T) => string | undefined) {
  return [...values].filter((value) => time(value)).sort((left, right) => time(right)!.localeCompare(time(left)!))[0];
}

export function displaySyncTime(value?: string) {
  if (!value) return '尚无';
  return new Intl.DateTimeFormat('zh-CN', {
    month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false,
  }).format(new Date(value));
}

function jobResult(job?: SyncJob) {
  if (!job) return '暂无结果';
  if (job.status === 'queued') return '已加入后端队列';
  if (job.status === 'running') return '正在从服务器获取邮件';
  if (job.status === 'failed') return job.errorMessage || '同步失败';
  if (job.status === 'cancelled') return '任务已取消';
  return `扫描 ${job.syncedCount ?? 0} · 新增 ${job.newCount ?? 0} · 更新 ${job.updatedCount ?? 0} · 删除 ${job.deletedCount ?? 0}`;
}

function statusPresentation(status: AccountSyncStatus) {
  const latestJob = newest(status.jobs, (job) => job.finishedAt ?? job.startedAt ?? job.createdAt);
  const failure = status.states.find((state) => state.connectionStatus === 'authRequired')
    ?? status.states.find((state) => state.lastErrorMessage);
  if (failure) return { tone: 'error', label: failure.connectionStatus === 'authRequired' ? '需要重新授权' : '同步异常', icon: <WarningCircle size={16} />, job: latestJob };
  if (status.jobs.some((job) => job.status === 'running')) return { tone: 'running', label: '正在同步', icon: <ArrowClockwise className="sync-policy-spin" size={16} />, job: latestJob };
  if (status.jobs.some((job) => job.status === 'queued')) return { tone: 'queued', label: '等待 Worker 执行', icon: <Clock size={16} />, job: latestJob };
  if (!status.policy.enabled) return { tone: 'paused', label: '自动同步已暂停', icon: <Clock size={16} />, job: latestJob };
  return { tone: 'success', label: latestJob?.status === 'succeeded' ? '最近同步成功' : '等待首次同步', icon: <CheckCircle size={16} />, job: latestJob };
}

export function AccountSyncSummary({ status }: { status: AccountSyncStatus }) {
  const presentation = statusPresentation(status);
  const lastAttempt = newest(status.states, (state) => state.lastAttemptAt)?.lastAttemptAt;
  const lastSuccess = newest(status.states, (state) => state.lastSuccessAt)?.lastSuccessAt;
  const nextSync = [...status.states].filter((state) => state.nextSyncAt).sort((left, right) => left.nextSyncAt!.localeCompare(right.nextSyncAt!))[0]?.nextSyncAt;
  return <div className={`account-sync-summary tone-${presentation.tone}`}>
    <header><span>{presentation.icon}<strong>{presentation.label}</strong></span><small>{status.policy.enabled ? `每 ${status.policy.intervalMinutes} 分钟` : '已暂停'}</small></header>
    <div className="account-sync-times">
      <span><small>最后尝试</small><strong>{displaySyncTime(lastAttempt ?? presentation.job?.startedAt)}</strong></span>
      <span><small>最后成功</small><strong>{displaySyncTime(lastSuccess)}</strong></span>
      <span><small>下次计划</small><strong>{status.policy.enabled ? displaySyncTime(nextSync) : '已暂停'}</strong></span>
    </div>
    <p title={jobResult(presentation.job)}><strong>最近结果</strong>{jobResult(presentation.job)}</p>
  </div>;
}

const roleLabels: Record<MailboxRole, string> = { inbox: '收件箱', sent: '已发送', archive: '归档', trash: '垃圾箱', custom: '自定义文件夹' };

function mailboxName(account: Account, state: MailboxSyncState) {
  return account.mailboxes.find((mailbox) => mailbox.path === state.mailbox)?.name
    ?? (state.mailbox.startsWith('@role:') ? roleLabels[state.mailboxRole] : state.mailbox);
}

function jobForState(jobs: SyncJob[], state: MailboxSyncState) {
  return newest(jobs.filter((job) => job.mailbox === state.mailbox || (!job.mailbox && job.mailboxRole === state.mailboxRole)), (job) => job.finishedAt ?? job.startedAt ?? job.createdAt);
}

const stateLabels: Record<MailboxSyncState['syncState'], string> = { idle: '已同步', scheduled: '已计划', running: '同步中', backoff: '等待重试', paused: '已暂停' };

export function MailboxSyncDetails({ account, status }: { account: Account; status: AccountSyncStatus }) {
  if (status.states.length === 0) return <div className="mailbox-sync-empty">尚无文件夹同步记录，可点击“立即同步”创建首次任务。</div>;
  return <section className="mailbox-sync-details">
    <header><strong>文件夹同步明细</strong><small>{status.states.length} 个同步目标</small></header>
    <div>{status.states.map((state) => {
      const job = jobForState(status.jobs, state);
      const failed = Boolean(state.lastErrorMessage);
      return <article key={`${state.mailboxRole}:${state.mailbox}`} className={failed ? 'has-error' : ''}>
        <header><span><strong>{mailboxName(account, state)}</strong><small>{state.mailbox}</small></span><em data-state={state.syncState}>{state.connectionStatus === 'authRequired' ? '需重新授权' : stateLabels[state.syncState]}</em></header>
        <dl><div><dt>最后尝试</dt><dd>{displaySyncTime(state.lastAttemptAt)}</dd></div><div><dt>最后成功</dt><dd>{displaySyncTime(state.lastSuccessAt)}</dd></div><div><dt>下次计划</dt><dd>{state.nextSyncAt ? displaySyncTime(state.nextSyncAt) : '—'}</dd></div></dl>
        <p title={state.lastErrorMessage ?? jobResult(job)}>{state.lastErrorMessage ? `失败：${state.lastErrorMessage}` : `结果：${jobResult(job)}`}</p>
      </article>;
    })}</div>
  </section>;
}
