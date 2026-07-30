import type { FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import type { SyncPolicy } from '../../types';
import { AppCheckbox, AppSelect } from '../../components/form-controls';

export function DefaultSyncPolicyForm({ policy, busy, onSubmit }: { policy: Omit<SyncPolicy, 'accountId' | 'updatedAt'>; busy: boolean; onSubmit: (event: FormEvent<HTMLFormElement>) => void }) {
  return <form className="sync-default-policy" onSubmit={onSubmit}>
    <header><span><strong>新账户默认同步策略</strong><small>新接入邮箱自动继承；已有账户仍使用各自设置。</small></span><Button appearance="primary" type="submit" disabled={busy}>{busy ? '保存中…' : '保存默认值'}</Button></header>
    <div className="sync-default-fields">
      <label className="sync-default-toggle"><AppCheckbox name="enabled" defaultChecked={policy.enabled} /><span><strong>后端自动同步</strong><small>新账户接入后默认启用</small></span></label>
      <label className="sync-default-select"><span>同步频率</span><AppSelect name="intervalMinutes" defaultValue={String(policy.intervalMinutes)} options={[{ value: '1', label: '每 1 分钟' }, { value: '5', label: '每 5 分钟' }, { value: '15', label: '每 15 分钟' }, { value: '30', label: '每 30 分钟' }, { value: '60', label: '每 60 分钟' }]} /></label>
      <label className="sync-default-select"><span>同步范围</span><AppSelect name="folderMode" defaultValue={policy.folderMode === 'selected' ? 'inbox' : policy.folderMode} options={[{ value: 'inbox', label: '仅收件箱' }, { value: 'standard', label: '收件箱、已发送和归档' }]} /></label>
    </div>
    <div className="sync-default-options"><label><AppCheckbox name="syncOnStart" defaultChecked={policy.syncOnStart} />服务启动后补同步</label><label><AppCheckbox name="retryOnRecovery" defaultChecked={policy.retryOnRecovery} />网络恢复后重试</label><label><AppCheckbox name="notifyOnError" defaultChecked={policy.notifyOnError} />持续失败时通知</label></div>
  </form>;
}
