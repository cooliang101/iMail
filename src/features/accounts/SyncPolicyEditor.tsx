import { Button } from '@fluentui/react-components';
import { ArrowClockwise, CheckCircle, WarningCircle } from '@phosphor-icons/react';
import { useState, type FormEvent } from 'react';
import { AppCheckbox, AppSelect } from '../../components/form-controls';
import type { Account, AccountSyncStatus } from '../../types';
import { MailboxSyncDetails } from './SyncStatusSummary';

function latest(values: Array<string | undefined>) { return values.filter((value): value is string => Boolean(value)).sort().at(-1); }
function earliest(values: Array<string | undefined>) { return values.filter((value): value is string => Boolean(value)).sort().at(0); }
function displayTime(value?: string) { return value ? new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(value)) : '尚未完成'; }

export function SyncPolicyEditor({ account, status, busy, onSave, onSync, onClose }: {
  account: Account;
  status: AccountSyncStatus;
  busy: boolean;
  onSave: (changes: Record<string, unknown>) => Promise<void>;
  onSync: () => Promise<void>;
  onClose: () => void;
}) {
  const [folderMode, setFolderMode] = useState(status.policy.folderMode);
  const running = status.jobs.some((job) => job.status === 'queued' || job.status === 'running');
  const lastSuccess = latest(status.states.map((state) => state.lastSuccessAt));
  const nextSync = earliest(status.states.map((state) => state.nextSyncAt));
  const failure = status.states.find((state) => state.connectionStatus === 'authRequired') ?? status.states.find((state) => state.lastErrorMessage);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    await onSave({
      enabled: form.has('enabled'), intervalMinutes: Number(form.get('intervalMinutes')), folderMode: form.get('folderMode'),
      selectedMailboxes: form.getAll('selectedMailboxes'), syncOnStart: form.has('syncOnStart'),
      retryOnRecovery: form.has('retryOnRecovery'), notifyOnError: form.has('notifyOnError'),
    });
  }

  return <form className="sync-policy-editor" onSubmit={(event) => void submit(event)}>
    <div className="sync-policy-status">
      <span>{failure ? <WarningCircle size={17} /> : running ? <ArrowClockwise className="sync-policy-spin" size={17} /> : <CheckCircle size={17} />}</span>
      <div><strong>{failure ? failure.connectionStatus === 'authRequired' ? '需要重新授权' : '同步遇到问题' : running ? '同步任务执行中' : status.policy.enabled ? '后端自动同步已启用' : '自动同步已暂停'}</strong>
        <small>{failure?.lastErrorMessage ?? `上次成功：${displayTime(lastSuccess)} · 下次计划：${status.policy.enabled ? displayTime(nextSync) : '已暂停'}`}</small></div>
    </div>
    <MailboxSyncDetails account={account} status={status} />
    <div className="sync-policy-grid">
      <label className="sync-toggle"><AppCheckbox name="enabled" defaultChecked={status.policy.enabled} /><span><strong>后端自动同步</strong><small>前端关闭后仍按策略执行</small></span></label>
      <label><span>同步频率</span><AppSelect name="intervalMinutes" defaultValue={String(status.policy.intervalMinutes)} options={[
        { value: '1', label: '每 1 分钟' }, { value: '5', label: '每 5 分钟' }, { value: '15', label: '每 15 分钟' },
        { value: '30', label: '每 30 分钟' }, { value: '60', label: '每 60 分钟' },
      ]} /></label>
      <label><span>同步范围</span><AppSelect name="folderMode" value={folderMode} onValueChange={(value) => setFolderMode(value as typeof folderMode)} options={[
        { value: 'inbox', label: '仅收件箱' }, { value: 'standard', label: '收件箱、已发送和归档' }, { value: 'selected', label: '收件箱和指定文件夹' },
      ]} /></label>
    </div>
    {folderMode === 'selected' && <fieldset className="sync-folder-options"><legend>额外同步文件夹</legend>{account.mailboxes.filter((folder) => folder.selectable && folder.path.toUpperCase() !== 'INBOX').map((folder) =>
      <label key={folder.path}><AppCheckbox name="selectedMailboxes" value={folder.path} defaultChecked={status.policy.selectedMailboxes.includes(folder.path)} /><span>{folder.name}</span><small>{folder.path}</small></label>)}</fieldset>}
    <div className="sync-policy-options">
      <label><AppCheckbox name="syncOnStart" defaultChecked={status.policy.syncOnStart} />服务启动后补同步</label>
      <label><AppCheckbox name="retryOnRecovery" defaultChecked={status.policy.retryOnRecovery} />网络恢复后重试</label>
      <label><AppCheckbox name="notifyOnError" defaultChecked={status.policy.notifyOnError} />持续失败时通知</label>
    </div>
    <footer className="sync-policy-actions"><button type="button" onClick={onClose}>关闭</button><button type="button" disabled={busy || running} onClick={() => void onSync()}><ArrowClockwise size={15} />{running ? '已在队列中' : '立即同步'}</button><Button appearance="primary" type="submit" disabled={busy}>{busy ? '保存中…' : '保存同步设置'}</Button></footer>
  </form>;
}
