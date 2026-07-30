import { useEffect, useRef, useState, type FormEvent } from 'react';
import { CaretDown, Envelope, WarningCircle } from '@phosphor-icons/react';
import { api } from '../../api';
import { subscribeSyncEvents } from '../../sync-events';
import { oauthCallbackOrigins } from '../../provider-guides';
import type { Account, AccountSyncStatus, SyncPolicy, SyncWorkerHealth } from '../../types';
import type { Notice } from '../../app-model';
import { AccountSettingsCard } from './AccountSettingsCard';
import { DefaultSyncPolicyForm } from './DefaultSyncPolicyForm';

export function AccountSettingsPanel({ accounts, section, onAddAccount, onReload, setNotice }: { accounts: Account[]; section: 'accounts' | 'sync'; onAddAccount: () => void; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  const [busyId, setBusyId] = useState<string | null>(null);
  const [credentialId, setCredentialId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [syncEditingId, setSyncEditingId] = useState<string | null>(null);
  const [syncStatuses, setSyncStatuses] = useState<AccountSyncStatus[]>([]);
  const [defaultPolicy, setDefaultPolicy] = useState<Omit<SyncPolicy, 'accountId' | 'updatedAt'> | null>(null);
  const [workerHealth, setWorkerHealth] = useState<SyncWorkerHealth | null>(null);
  const [confirmRemoveId, setConfirmRemoveId] = useState<string | null>(null);
  const [advancedSyncOpen, setAdvancedSyncOpen] = useState(false);
  const [error, setError] = useState('');
  const popupRef = useRef<Window | null>(null);
  const oauthOriginsRef = useRef(oauthCallbackOrigins(['http://localhost:8787/api/oauth'], window.location.origin));
  const providersRequestedRef = useRef(false);
  const defaultPolicyRequestedRef = useRef(false);

  useEffect(() => {
    setCredentialId(null); setEditingId(null); setSyncEditingId(null); setConfirmRemoveId(null); setAdvancedSyncOpen(false); setError('');
  }, [section]);

  useEffect(() => {
    if (!providersRequestedRef.current) {
      providersRequestedRef.current = true;
      void api<{ oauth: Array<{ redirectUri: string }> }>('/api/providers').then((result) => {
        oauthOriginsRef.current = oauthCallbackOrigins(result.oauth.map((item) => item.redirectUri), window.location.origin);
      }).catch(() => undefined);
    }
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

  useEffect(() => {
    if (section !== 'sync') return;
    const applyStatus = (event: MessageEvent) => {
      try {
        const result = JSON.parse(event.data) as { accounts: AccountSyncStatus[]; worker: SyncWorkerHealth };
        setSyncStatuses(result.accounts); setWorkerHealth(result.worker);
      } catch {
        // Ignore a malformed status snapshot and wait for the next SSE heartbeat.
      }
    };
    if (!defaultPolicyRequestedRef.current) {
      defaultPolicyRequestedRef.current = true;
      void api<{ policy: Omit<SyncPolicy, 'accountId' | 'updatedAt'> }>('/api/sync-policy')
        .then((result) => setDefaultPolicy(result.policy)).catch(() => undefined);
    }
    return subscribeSyncEvents(['sync.status'], applyStatus);
  }, [section]);

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
      const result = await api<{ policy: SyncPolicy }>(`/api/accounts/${account.id}/sync-policy`, { method: 'PATCH', body: JSON.stringify(changes) });
      setSyncStatuses((current) => current.map((status) => status.accountId === account.id ? { ...status, policy: result.policy } : status));
      setNotice({ kind: 'success', text: `${account.email} 的后端同步策略已更新` });
    } catch (value) { setError(value instanceof Error ? value.message : '同步策略更新失败'); }
    finally { setBusyId(null); }
  }

  async function queueSync(account: Account) {
    setBusyId(account.id); setError('');
    try {
      await api(`/api/accounts/${account.id}/sync`, { method: 'POST' });
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

  return <section className="settings-feature-panel">
    <header className="settings-panel-heading"><div><span>{section === 'accounts' ? '连接与身份' : '后台同步'}</span><h2>{section === 'accounts' ? '邮箱管理' : '同步'}</h2><p>{section === 'accounts' ? '管理邮箱资料、授权状态与本地连接。' : '设置后台同步频率、范围与失败恢复策略。'}</p></div></header>
    <div className="settings-panel-body">
    {section === 'sync' && workerHealth && <div className={`sync-worker-health ${workerOnline ? 'is-online' : 'is-offline'}`}><span>{workerOnline ? '同步 Worker 运行正常' : '同步 Worker 未运行或心跳已过期'}</span><small>{workerHealth.queuedJobs > 0 ? `${workerHealth.queuedJobs} 个任务正在等待` : '当前没有积压任务'}</small></div>}
    {section === 'sync' && defaultPolicy && <DefaultSyncPolicyForm policy={defaultPolicy} busy={busyId === 'defaults'} onSubmit={(event) => void updateDefaultSyncPolicy(event)} />}
    {section === 'sync' && <button type="button" className={`sync-advanced-toggle ${advancedSyncOpen ? 'is-open' : ''}`} aria-expanded={advancedSyncOpen} aria-controls="account-sync-advanced" onClick={() => setAdvancedSyncOpen((open) => !open)}><span><strong>高级设置</strong><small>按邮箱单独配置同步策略</small></span><em>{accounts.length} 个邮箱</em><CaretDown size={18} weight="bold" /></button>}
    {(section === 'accounts' || advancedSyncOpen) && <div id={section === 'sync' ? 'account-sync-advanced' : undefined} className={section === 'sync' ? 'sync-advanced-content' : undefined}>
    {accounts.length === 0 ? <div className="settings-empty"><Envelope size={38} weight="duotone" /><h3>还没有真实邮箱</h3><p>接入第一个邮箱后，即可在这里管理账户和同步策略。</p><button type="button" className="settings-primary-action" onClick={onAddAccount}>添加邮箱</button></div> : <div className="settings-account-list">
      {accounts.map((account) => <AccountSettingsCard key={account.id} account={account} section={section} syncStatus={syncStatuses.find((item) => item.accountId === account.id)} workspaceOptions={workspaceOptions} busy={busyId === account.id} editing={editingId === account.id} credentialOpen={credentialId === account.id} removeConfirmOpen={confirmRemoveId === account.id} syncEditing={syncEditingId === account.id}
        onEdit={() => { setSyncEditingId(null); setCredentialId(null); setConfirmRemoveId(null); setEditingId(account.id); }} onCancelEdit={() => setEditingId(null)} onUpdateProfile={(event) => void updateProfile(event, account)} onRetry={() => void retryConnection(account)} onReconnect={() => void reconnect(account)}
        onOpenCredential={() => { setSyncEditingId(null); setEditingId(null); setConfirmRemoveId(null); setCredentialId(account.id); }} onCloseCredential={() => setCredentialId(null)} onUpdateCredential={(event) => void updateCredential(event, account)} onOpenRemove={() => { setSyncEditingId(null); setEditingId(null); setCredentialId(null); setConfirmRemoveId(account.id); }} onCloseRemove={() => setConfirmRemoveId(null)} onRemove={() => void remove(account)}
        onOpenSync={() => { setCredentialId(null); setConfirmRemoveId(null); setSyncEditingId(account.id); }} onCloseSync={() => setSyncEditingId(null)} onSaveSync={(changes) => updateSyncPolicy(account, changes)} onQueueSync={() => queueSync(account)} />)}
    </div>}
    </div>}
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}</div>
  </section>;
}
