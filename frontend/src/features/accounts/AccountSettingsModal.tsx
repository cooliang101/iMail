import { useEffect, useRef, useState, type FormEvent } from 'preact/compat';
import { Envelope, WarningCircle } from '../../components/icons';
import { api } from '../../api';
import { oauthCallbackOrigins } from '../../provider-guides';
import type { Account } from '../../types';
import type { Notice } from '../../app-model';
import { AccountSettingsCard } from './AccountSettingsCard';
import { proxyInputFromForm } from './ProxyFields';
import { usePlatform } from '../../platform/runtime';

export function AccountSettingsPanel({ accounts, onAddAccount, onReload, setNotice }: { accounts: Account[]; onAddAccount: () => void; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  const platform = usePlatform();
  const [busyId, setBusyId] = useState<string | null>(null);
  const [credentialId, setCredentialId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [proxyEditingId, setProxyEditingId] = useState<string | null>(null);
  const [confirmRemoveId, setConfirmRemoveId] = useState<string | null>(null);
  const [error, setError] = useState('');
  const popupRef = useRef<Window | null>(null);
  const oauthOriginsRef = useRef(oauthCallbackOrigins(['http://localhost:8787/api/oauth'], window.location.origin));
  const providersRequestedRef = useRef(false);
  const oauthAttemptRef = useRef(0);

  useEffect(() => {
    if (!providersRequestedRef.current) {
      providersRequestedRef.current = true;
      void api<{ oauth: Array<{ redirectUri: string }> }>('/api/providers').then((result) => {
        oauthOriginsRef.current = oauthCallbackOrigins(result.oauth.map((item) => item.redirectUri), window.location.origin);
      }).catch((reason) => setError(reason instanceof Error ? reason.message : '快捷登录配置读取失败'));
    }
    const receive = (event: MessageEvent) => {
      if (event.source !== popupRef.current || !oauthOriginsRef.current.has(event.origin) || event.data?.source !== 'imail-oauth') return;
      popupRef.current = null;
      setBusyId(null);
      if (event.data.success) {
        void onReload().then(() => setNotice(event.data.warning
          ? { kind: 'error', text: `授权已保存，连接验证失败：${event.data.warning}` }
          : { kind: 'success', text: '邮箱授权已更新' }))
          .catch((reason) => setError(reason instanceof Error ? reason.message : '授权已完成，但邮箱列表刷新失败'));
      } else setError(event.data.message || '重新授权未完成');
    };
    window.addEventListener('message', receive);
    const timer = window.setInterval(() => {
      if (popupRef.current?.closed) { popupRef.current = null; setBusyId(null); setError('授权窗口已关闭；如果已经完成授权，请点击“重试连接”确认状态。'); }
    }, 700);
    return () => { window.removeEventListener('message', receive); window.clearInterval(timer); };
  }, [onReload, setNotice]);

  useEffect(() => () => { oauthAttemptRef.current += 1; popupRef.current?.close(); }, []);

  async function reconnect(account: Account) {
    setError('');
    if (platform.kind === 'tauri') {
      const attempt = ++oauthAttemptRef.current;
      setBusyId(account.id);
      try {
        const result = await api<{ authorizationUrl: string; state: string }>(`/api/accounts/${account.id}/oauth/reconnect`, { method: 'POST' });
        await platform.openExternal(result.authorizationUrl);
        const deadline = Date.now() + 10 * 60_000;
        while (oauthAttemptRef.current === attempt && Date.now() < deadline) {
          await new Promise((resolve) => window.setTimeout(resolve, 1_000));
          const status = await api<{ completed: false } | { completed: true; account: Account }>('/api/oauth/status', { method: 'POST', body: JSON.stringify({ state: result.state }) });
          if (!status.completed) continue;
          await onReload();
          setNotice(status.account.status === 'error'
            ? { kind: 'error', text: `授权已保存，连接验证失败：${status.account.lastError || '邮件连接仍需重试'}` }
            : { kind: 'success', text: '邮箱授权已更新' });
          return;
        }
        if (oauthAttemptRef.current === attempt) setError('授权等待已超时，请重新发起登录。');
      } catch (value) {
        if (oauthAttemptRef.current === attempt) setError(value instanceof Error ? value.message : typeof value === 'string' ? value : '无法开始重新授权');
      } finally {
        if (oauthAttemptRef.current === attempt) setBusyId(null);
      }
      return;
    }
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
      setError(value instanceof Error ? value.message : typeof value === 'string' ? value : '无法开始重新授权');
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

  async function updateProxy(event: FormEvent<HTMLFormElement>, account: Account) {
    event.preventDefault(); setBusyId(account.id); setError('');
    try {
      await api(`/api/accounts/${account.id}/proxy`, { method: 'PUT', body: JSON.stringify(proxyInputFromForm(new FormData(event.currentTarget))) });
      setProxyEditingId(null);
      await onReload();
      setNotice({ kind: 'success', text: `${account.email} 的网络代理已验证并保存` });
    } catch (value) { setError(value instanceof Error ? value.message : '网络代理更新失败'); }
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

  const workspaceOptions = Array.from(new Set(accounts.map((account) => account.group)))
    .map((group) => ({ value: group, label: group }));

  return <section className="settings-feature-panel">
    <header className="settings-panel-heading"><div><span>连接与身份</span><h2>邮箱管理</h2><p>按邮箱管理资料、授权状态、网络代理与工作空间。</p></div></header>
    <div className="settings-panel-body">
    {accounts.length === 0 ? <div className="settings-empty"><Envelope size={38} weight="duotone" /><h3>还没有真实邮箱</h3><p>接入第一个邮箱后，即可在这里查看邮件接收状态。</p><button type="button" className="settings-primary-action" onClick={onAddAccount}>添加邮箱</button></div> : <div className="settings-account-list">
      {accounts.map((account) => <AccountSettingsCard key={account.id} account={account} proxyPresets={accounts.filter((candidate) => candidate.id !== account.id && candidate.proxy).map((candidate) => ({ accountId: candidate.id, label: `${candidate.displayName} · ${candidate.email}`, proxy: candidate.proxy! }))} workspaceOptions={workspaceOptions} busy={busyId === account.id} editing={editingId === account.id} proxyEditing={proxyEditingId === account.id} credentialOpen={credentialId === account.id} removeConfirmOpen={confirmRemoveId === account.id}
        onEdit={() => { setProxyEditingId(null); setCredentialId(null); setConfirmRemoveId(null); setEditingId(account.id); }} onCancelEdit={() => setEditingId(null)} onUpdateProfile={(event) => void updateProfile(event, account)}
        onOpenProxy={() => { setEditingId(null); setCredentialId(null); setConfirmRemoveId(null); setProxyEditingId(account.id); }} onCloseProxy={() => setProxyEditingId(null)} onUpdateProxy={(event) => void updateProxy(event, account)} onRetry={() => void retryConnection(account)} onReconnect={() => void reconnect(account)}
        onOpenCredential={() => { setEditingId(null); setProxyEditingId(null); setConfirmRemoveId(null); setCredentialId(account.id); }} onCloseCredential={() => setCredentialId(null)} onUpdateCredential={(event) => void updateCredential(event, account)} onOpenRemove={() => { setEditingId(null); setProxyEditingId(null); setCredentialId(null); setConfirmRemoveId(account.id); }} onCloseRemove={() => setConfirmRemoveId(null)} onRemove={() => void remove(account)} />)}
    </div>}
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}</div>
  </section>;
}
