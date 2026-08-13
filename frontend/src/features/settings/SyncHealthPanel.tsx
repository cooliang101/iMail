import { useCallback, useEffect, useMemo, useState } from 'react';
import { ArrowClockwise, CheckCircle, Clock, Pulse, WarningCircle } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Account, AccountSyncStatus, SyncWorkerHealth } from '../../types';
import { PanelHeading } from './PanelHeading';

type SyncStatusResponse = { accounts: AccountSyncStatus[]; worker: SyncWorkerHealth };

function formatTime(value?: string) {
  if (!value) return '暂无记录';
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(date);
}

export function SyncHealthPanel({ accounts }: { accounts: Account[] }) {
  const [status, setStatus] = useState<SyncStatusResponse>();
  const [error, setError] = useState('');
  const [busyAccount, setBusyAccount] = useState<string>();
  const [loading, setLoading] = useState(true);
  const inspect = useCallback(async () => {
    setLoading(true); setError('');
    try { setStatus(await api<SyncStatusResponse>('/api/sync-status')); }
    catch (reason) { setError(reason instanceof Error ? reason.message : '同步状态读取失败'); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void inspect(); }, [inspect]);
  const failures = useMemo(() => status?.accounts.flatMap((account) => account.states.filter((state) => state.lastErrorMessage || state.connectionStatus !== 'connected')) ?? [], [status]);
  async function retry(accountId: string) {
    setBusyAccount(accountId); setError('');
    try { await api(`/api/accounts/${accountId}/sync`, { method: 'POST' }); await inspect(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : '同步重试失败'); }
    finally { setBusyAccount(undefined); }
  }
  return <section className="settings-feature-panel"><PanelHeading eyebrow="运行诊断" title="同步健康" description="查看每个邮箱的最近同步结果、退避状态和后台任务，并可直接重新排队。" syncNote={false} />
    <div className="settings-panel-body sync-health-panel"><div className="sync-health-summary" aria-live="polite">
      <div><Pulse size={22} /><span><small>后台 Worker</small><strong>{status?.worker.workers.length ?? 0} 个活跃</strong></span></div>
      <div><Clock size={22} /><span><small>排队任务</small><strong>{status?.worker.queuedJobs ?? 0} 个</strong></span></div>
      <div className={failures.length ? 'has-error' : ''}>{failures.length ? <WarningCircle size={22} /> : <CheckCircle size={22} />}<span><small>需要处理</small><strong>{failures.length} 个文件夹</strong></span></div>
    </div>{error && <div className="sync-health-error" role="alert"><WarningCircle size={18} />{error}</div>}
    <div className="sync-health-actions"><button type="button" onClick={() => void inspect()} disabled={loading}><ArrowClockwise className={loading ? 'sync-health-spin' : ''} size={16} />刷新状态</button></div>
    <div className="sync-health-accounts">{accounts.length === 0 ? <div className="settings-empty"><h3>还没有邮箱</h3><p>接入邮箱后，这里会显示同步健康和恢复入口。</p></div> : accounts.map((account) => {
      const current = status?.accounts.find((item) => item.accountId === account.id);
      const lastSuccess = current?.states.map((state) => state.lastSuccessAt).filter(Boolean).sort().at(-1);
      const nextRetry = current?.states.map((state) => state.nextSyncAt).filter(Boolean).sort().at(0);
      const failed = current?.states.filter((state) => state.lastErrorMessage || state.connectionStatus !== 'connected') ?? [];
      const running = current?.states.filter((state) => state.syncState === 'running' || state.syncState === 'scheduled').length ?? 0;
      return <article key={account.id} className={failed.length ? 'has-error' : ''}><header><div><strong>{account.displayName}</strong><small>{account.email}</small></div><span>{failed.length ? `${failed.length} 项异常` : running ? `${running} 项同步中` : '运行正常'}</span></header>
        <dl><div><dt>最近成功</dt><dd>{formatTime(lastSuccess)}</dd></div><div><dt>下次重试</dt><dd>{formatTime(nextRetry)}</dd></div><div><dt>最近任务</dt><dd>{current?.jobs[0]?.status ?? '暂无'}</dd></div></dl>
        {failed.slice(0, 2).map((state) => <p key={state.mailbox}><strong>{state.mailbox}</strong>{state.lastErrorMessage ?? (state.connectionStatus === 'authRequired' ? '需要重新授权或更新凭据' : '邮箱服务暂时不可达')}</p>)}
        <button type="button" onClick={() => void retry(account.id)} disabled={busyAccount === account.id}>{busyAccount === account.id ? '正在排队…' : '重新同步此邮箱'}</button></article>;
    })}</div></div></section>;
}
