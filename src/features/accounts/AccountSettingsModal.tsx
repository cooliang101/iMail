import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowClockwise, Envelope, Key, PencilSimple, SlidersHorizontal, Trash, WarningCircle, X } from '@phosphor-icons/react';
import { api } from '../../api';
import { credentialGuideFor, oauthCallbackOrigins } from '../../provider-guides';
import type { Account, AccountSyncStatus, SyncPolicy, SyncWorkerHealth } from '../../types';
import type { Notice } from '../../app-model';
import { Overlay, ProviderIcon, providerLabel } from '../../components/shared';
import { AppCheckbox, AppInput, AppSelect } from '../../components/form-controls';
import { SyncPolicyEditor } from './SyncPolicyEditor';
import { AccountSyncSummary } from './SyncStatusSummary';

export function AccountSettingsModal({ accounts, onClose, onReload, setNotice }: { accounts: Account[]; onClose: () => void; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  const [busyId, setBusyId] = useState<string | null>(null);
  const [credentialId, setCredentialId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [syncEditingId, setSyncEditingId] = useState<string | null>(null);
  const [syncStatuses, setSyncStatuses] = useState<AccountSyncStatus[]>([]);
  const [defaultPolicy, setDefaultPolicy] = useState<Omit<SyncPolicy, 'accountId' | 'updatedAt'> | null>(null);
  const [workerHealth, setWorkerHealth] = useState<SyncWorkerHealth | null>(null);
  const [confirmRemoveId, setConfirmRemoveId] = useState<string | null>(null);
  const [error, setError] = useState('');
  const popupRef = useRef<Window | null>(null);
  const oauthOriginsRef = useRef(oauthCallbackOrigins(['http://localhost:8787/api/oauth'], window.location.origin));

  useEffect(() => {
    void api<{ oauth: Array<{ redirectUri: string }> }>('/api/providers').then((result) => {
      oauthOriginsRef.current = oauthCallbackOrigins(result.oauth.map((item) => item.redirectUri), window.location.origin);
    }).catch(() => undefined);
    const receive = (event: MessageEvent) => {
      if (event.source !== popupRef.current || !oauthOriginsRef.current.has(event.origin) || event.data?.source !== 'imail-oauth') return;
      popupRef.current = null;
      setBusyId(null);
      if (event.data.success) {
        void onReload().then(() => setNotice(event.data.warning
          ? { kind: 'error', text: `授权已保存，连接验证失败：${event.data.warning}` }
          : { kind: 'success', text: '邮箱授权已更新' }));
      } else setError(event.data.message || '重新授权未完成');
    };
    window.addEventListener('message', receive);
    const timer = window.setInterval(() => {
      if (popupRef.current?.closed) { popupRef.current = null; setBusyId(null); setError('授权窗口已关闭；如果已经完成授权，请点击“重试连接”确认状态。'); }
    }, 700);
    return () => { window.removeEventListener('message', receive); window.clearInterval(timer); };
  }, [onReload, setNotice]);

  async function refreshSyncStatus() {
    const [result, defaults] = await Promise.all([
      api<{ accounts: AccountSyncStatus[]; worker: SyncWorkerHealth }>('/api/sync-status'),
      api<{ policy: Omit<SyncPolicy, 'accountId' | 'updatedAt'> }>('/api/sync-policy'),
    ]);
    setSyncStatuses(result.accounts); setWorkerHealth(result.worker); setDefaultPolicy(defaults.policy);
  }

  useEffect(() => {
    void refreshSyncStatus().catch(() => undefined);
    const timer = window.setInterval(() => { void refreshSyncStatus().catch(() => undefined); }, 5_000);
    return () => window.clearInterval(timer);
  }, [accounts.length]);

  async function reconnect(account: Account) {
    setError('');
    const popup = window.open('', 'imail-oauth', 'popup,width=560,height=720,menubar=no,toolbar=no');
    if (!popup) { setError('浏览器阻止了登录窗口，请允许弹出窗口后重试'); return; }
    popup.document.write('<title>iMail</title><p style="font-family:system-ui;padding:32px">正在打开安全登录…</p>');
    popupRef.current = popup;
    setBusyId(account.id);
    try {
      const result = await api<{ authorizationUrl: string }>(`/api/accounts/${account.id}/oauth/reconnect`, { method: 'POST' });
      popup.location.replace(result.authorizationUrl);
    } catch (value) {
      popup.close(); popupRef.current = null; setBusyId(null);
      setError(value instanceof Error ? value.message : '无法开始重新授权');
    }
  }

  async function retryConnection(account: Account) {
    setBusyId(account.id); setError('');
    try {
      const result = await api<{ account: Account }>(`/api/accounts/${account.id}/connection-test`, { method: 'POST' });
      await onReload();
      if (result.account.status === 'connected') setNotice({ kind: 'success', text: `${account.email} 已使用现有授权恢复连接` });
      else setError(result.account.lastError || '连接验证失败，已保留现有授权');
    } catch (value) {
      setError(value instanceof Error ? value.message : '连接验证失败');
    } finally { setBusyId(null); }
  }

  async function updateCredential(event: FormEvent<HTMLFormElement>, account: Account) {
    event.preventDefault(); setBusyId(account.id); setError('');
    const form = new FormData(event.currentTarget);
    try {
      await api(`/api/accounts/${account.id}/credential`, { method: 'PUT', body: JSON.stringify({ password: form.get('password') }) });
      setCredentialId(null);
      await onReload();
      setNotice({ kind: 'success', text: `${account.email} 的授权凭据已更新并验证` });
    } catch (value) {
      setError(value instanceof Error ? value.message : '授权凭据更新失败');
    } finally { setBusyId(null); }
  }

  async function updateProfile(event: FormEvent<HTMLFormElement>, account: Account) {
    event.preventDefault(); setBusyId(account.id); setError('');
    const form = new FormData(event.currentTarget);
    const group = String(form.get('group'));
    const groupIcon = accounts.find((item) => item.group === group)?.groupIcon ?? 'folder';
    try {
      await api(`/api/accounts/${account.id}`, { method: 'PATCH', body: JSON.stringify({ displayName: form.get('displayName'), group, groupIcon }) });
      setEditingId(null);
      await onReload();
      setNotice({ kind: 'success', text: `${account.email} 的显示信息已更新` });
    } catch (value) { setError(value instanceof Error ? value.message : '邮箱信息更新失败'); }
    finally { setBusyId(null); }
  }

  async function remove(account: Account) {
    setBusyId(account.id); setError('');
    try {
      await api(`/api/accounts/${account.id}`, { method: 'DELETE' });
      setConfirmRemoveId(null);
      await onReload();
      setNotice({ kind: 'success', text: `${account.displayName} 已从本机移除` });
    } catch (value) { setError(value instanceof Error ? value.message : '移除失败'); }
    finally { setBusyId(null); }
  }

  async function updateSyncPolicy(account: Account, changes: Record<string, unknown>) {
    setBusyId(account.id); setError('');
    try {
      await api(`/api/accounts/${account.id}/sync-policy`, { method: 'PATCH', body: JSON.stringify(changes) });
      await refreshSyncStatus();
      setNotice({ kind: 'success', text: `${account.email} 的后端同步策略已更新` });
    } catch (value) { setError(value instanceof Error ? value.message : '同步策略更新失败'); }
    finally { setBusyId(null); }
  }

  async function queueSync(account: Account) {
    setBusyId(account.id); setError('');
    try {
      await api(`/api/accounts/${account.id}/sync`, { method: 'POST' });
      await refreshSyncStatus();
      setNotice({ kind: 'success', text: `${account.email} 已加入后端同步队列` });
    } catch (value) { setError(value instanceof Error ? value.message : '无法创建同步任务'); }
    finally { setBusyId(null); }
  }

  async function updateDefaultSyncPolicy(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusyId('defaults'); setError('');
    const form = new FormData(event.currentTarget);
    try {
      const result = await api<{ policy: Omit<SyncPolicy, 'accountId' | 'updatedAt'> }>('/api/sync-policy', { method: 'PATCH', body: JSON.stringify({
        enabled: form.has('enabled'), intervalMinutes: Number(form.get('intervalMinutes')), folderMode: form.get('folderMode'), selectedMailboxes: [],
        syncOnStart: form.has('syncOnStart'), retryOnRecovery: form.has('retryOnRecovery'), notifyOnError: form.has('notifyOnError'),
      }) });
      setDefaultPolicy(result.policy); setNotice({ kind: 'success', text: '新账户默认同步策略已保存' });
    } catch (value) { setError(value instanceof Error ? value.message : '默认同步策略更新失败'); }
    finally { setBusyId(null); }
  }

  const workspaceOptions = Array.from(new Set(accounts.map((account) => account.group)))
    .map((group) => ({ value: group, label: group }));

  const workerOnline = workerHealth?.workers.some((worker) => Date.now() - new Date(worker.heartbeatAt).getTime() < 30_000) ?? false;

  return <Overlay onClose={onClose} wide dialogClassName="account-settings-shell"><section className="account-settings-modal">
    <div className="modal-header"><div><span>账户资料与授权</span><h2>邮箱设置</h2><p>编辑显示名称和工作空间，检查连接或更新账户授权。</p></div><button type="button" aria-label="关闭邮箱设置" onClick={onClose}><X size={21} /></button></div>
    {workerHealth && <div className={`sync-worker-health ${workerOnline ? 'is-online' : 'is-offline'}`}><span>{workerOnline ? '同步 Worker 运行正常' : '同步 Worker 未运行或心跳已过期'}</span><small>{workerHealth.queuedJobs > 0 ? `${workerHealth.queuedJobs} 个任务正在等待` : '当前没有积压任务'}</small></div>}
    {defaultPolicy && <form className="sync-default-policy" onSubmit={(event) => void updateDefaultSyncPolicy(event)}>
      <header><span><strong>新账户默认同步策略</strong><small>新接入邮箱自动继承；已有账户仍使用各自设置。</small></span><Button appearance="primary" type="submit" disabled={busyId === 'defaults'}>{busyId === 'defaults' ? '保存中…' : '保存默认值'}</Button></header>
      <div className="sync-default-fields">
        <label className="sync-default-toggle"><AppCheckbox name="enabled" defaultChecked={defaultPolicy.enabled} /><span><strong>后端自动同步</strong><small>新账户接入后默认启用</small></span></label>
        <label className="sync-default-select"><span>同步频率</span><AppSelect name="intervalMinutes" defaultValue={String(defaultPolicy.intervalMinutes)} options={[{ value: '1', label: '每 1 分钟' }, { value: '5', label: '每 5 分钟' }, { value: '15', label: '每 15 分钟' }, { value: '30', label: '每 30 分钟' }, { value: '60', label: '每 60 分钟' }]} /></label>
        <label className="sync-default-select"><span>同步范围</span><AppSelect name="folderMode" defaultValue={defaultPolicy.folderMode === 'selected' ? 'inbox' : defaultPolicy.folderMode} options={[{ value: 'inbox', label: '仅收件箱' }, { value: 'standard', label: '收件箱、已发送和归档' }]} /></label>
      </div>
      <div className="sync-default-options"><label><AppCheckbox name="syncOnStart" defaultChecked={defaultPolicy.syncOnStart} />服务启动后补同步</label><label><AppCheckbox name="retryOnRecovery" defaultChecked={defaultPolicy.retryOnRecovery} />网络恢复后重试</label><label><AppCheckbox name="notifyOnError" defaultChecked={defaultPolicy.notifyOnError} />持续失败时通知</label></div>
    </form>}
    {accounts.length === 0 ? <div className="settings-empty"><Envelope size={38} weight="duotone" /><h3>还没有真实邮箱</h3><p>关闭设置后，点击左侧的加号接入第一个邮箱。</p></div> : <div className="settings-account-list">
      {accounts.map((account) => {
        const editing = editingId === account.id;
        const connectionText = account.status === 'connected' ? '连接正常' : account.status === 'syncing' ? '正在同步' : account.lastError || '连接异常';
        const authText = account.authMethod === 'oauth2' ? 'OAuth 2.0' : '授权码 / 专用密码';
        const syncStatus = syncStatuses.find((item) => item.accountId === account.id);

        return <article key={account.id} className={`settings-account-card ${editing ? 'is-editing' : ''} ${syncEditingId === account.id ? 'is-sync-editing' : ''}`}>
          {editing ? <form className="account-inline-editor" onSubmit={(event) => void updateProfile(event, account)}>
            <header className="settings-account-summary">
              <i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i>
              <span>
                <label className="account-name-editor"><strong>{providerLabel[account.provider]} ·</strong><span className="sr-only">显示名称</span><AppInput name="displayName" defaultValue={account.displayName} maxLength={80} autoFocus required /></label>
                <label className="account-workspace-editor"><small>{account.email} ·</small><span className="sr-only">所属工作空间</span><AppSelect name="group" defaultValue={account.group} options={workspaceOptions} /></label>
                <em className={`connection-${account.status}`}>{connectionText}</em>
              </span>
              <small className="account-auth-kind">{authText}</small>
            </header>
            <footer className="settings-account-actions card-editor-actions"><button type="button" onClick={() => setEditingId(null)}>取消</button><Button appearance="primary" type="submit" disabled={busyId === account.id}>{busyId === account.id ? '保存中…' : '保存修改'}</Button></footer>
          </form> : <>
            <header className="settings-account-summary"><i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i><span><strong>{providerLabel[account.provider]} · {account.displayName}</strong><small>{account.email} · {account.group}</small><em className={`connection-${account.status}`}>{connectionText}</em></span><small className="account-auth-kind">{authText}</small></header>
            {syncStatus && <AccountSyncSummary status={syncStatus} />}
            {syncEditingId === account.id && syncStatus ? <SyncPolicyEditor account={account} status={syncStatus} busy={busyId === account.id} onSave={(changes) => updateSyncPolicy(account, changes)} onSync={() => queueSync(account)} onClose={() => setSyncEditingId(null)} />
              : credentialId === account.id ? <form className="credential-renewal" onSubmit={(event) => void updateCredential(event, account)}><label><span>{credentialGuideFor(account.provider)?.secretLabel || '新的授权码 / 应用专用密码'}</span><AppInput name="password" type="password" placeholder={credentialGuideFor(account.provider)?.secretPlaceholder || '输入新的专用凭据'} autoFocus required /></label><div className="card-editor-actions"><button type="button" onClick={() => setCredentialId(null)}>取消</button><Button appearance="primary" type="submit" disabled={busyId === account.id}>{busyId === account.id ? '正在验证…' : '验证并更新'}</Button></div></form>
              : confirmRemoveId === account.id ? <div className="account-remove-confirm"><span><strong>确认移除这个邮箱？</strong><small>{account.email} 的本地邮件缓存也会删除。</small></span><button type="button" onClick={() => setConfirmRemoveId(null)}>取消</button><button type="button" className="confirm-remove-account" disabled={busyId === account.id} onClick={() => void remove(account)}>{busyId === account.id ? '正在移除…' : '确认移除'}</button></div>
                : <footer className="settings-account-actions"><button type="button" className="sync-settings-account" disabled={busyId === account.id || !syncStatus} onClick={() => { setCredentialId(null); setConfirmRemoveId(null); setSyncEditingId(account.id); }}><SlidersHorizontal size={15} />同步设置</button><button type="button" className="edit-account" disabled={busyId === account.id} onClick={() => { setSyncEditingId(null); setCredentialId(null); setConfirmRemoveId(null); setEditingId(account.id); }}><PencilSimple size={15} />编辑信息</button><button type="button" className="retry-account" disabled={busyId === account.id} onClick={() => void retryConnection(account)}><ArrowClockwise size={15} />{busyId === account.id ? '正在检查' : '重试连接'}</button>{account.authMethod === 'oauth2' ? <button type="button" className="reconnect-account" disabled={busyId === account.id} onClick={() => void reconnect(account)}><Key size={15} />重新授权</button> : <button type="button" className="reconnect-account" disabled={busyId === account.id} onClick={() => { setSyncEditingId(null); setEditingId(null); setConfirmRemoveId(null); setCredentialId(account.id); }}><Key size={15} />更新凭据</button>}<button type="button" className="remove-account" disabled={busyId === account.id} onClick={() => { setSyncEditingId(null); setEditingId(null); setCredentialId(null); setConfirmRemoveId(account.id); }}><Trash size={15} />移除</button></footer>}
          </>}
        </article>;
      })}
    </div>}
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
    <div className="modal-footer"><Button appearance="primary" onClick={onClose}>完成</Button></div>
  </section></Overlay>;
}
