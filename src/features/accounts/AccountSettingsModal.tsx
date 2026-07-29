import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowClockwise, ArrowRight, CaretDown, Check, Envelope, Gear, Key, Trash, WarningCircle, X } from '@phosphor-icons/react';
import { api } from '../../api';
import { credentialGuideFor, oauthCallbackOrigins } from '../../provider-guides';
import type { Account, ProviderId } from '../../types';
import type { Notice } from '../../app-model';
import { AccountProviderMark, Overlay, ProviderIcon, providerLabel, providers } from '../../components/shared';

export function AccountSettingsModal({ accounts, onClose, onReload, setNotice }: { accounts: Account[]; onClose: () => void; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  const [busyId, setBusyId] = useState<string | null>(null);
  const [credentialId, setCredentialId] = useState<string | null>(null);
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

  async function remove(account: Account) {
    if (!window.confirm(`确定从 iMail 移除 ${account.email}？本地邮件缓存也会一并删除。`)) return;
    setBusyId(account.id); setError('');
    try {
      await api(`/api/accounts/${account.id}`, { method: 'DELETE' });
      await onReload();
      setNotice({ kind: 'success', text: `${account.displayName} 已从本机移除` });
    } catch (value) { setError(value instanceof Error ? value.message : '移除失败'); }
    finally { setBusyId(null); }
  }

  return <Overlay onClose={onClose}><section className="account-settings-modal">
    <div className="modal-header"><div><span>连接与授权</span><h2>邮箱设置</h2><p>查看连接状态，更新 OAuth 授权或移除本地账户。</p></div><button type="button" aria-label="关闭邮箱设置" onClick={onClose}><X size={21} /></button></div>
    {accounts.length === 0 ? <div className="settings-empty"><Envelope size={38} weight="duotone" /><h3>还没有真实邮箱</h3><p>关闭设置后，点击左侧的加号接入第一个邮箱。</p></div> : <div className="settings-account-list">
      {accounts.map((account) => <div key={account.id} className="settings-account-block"><article className="settings-account">
        <i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i>
        <span><strong>{providerLabel[account.provider]} · {account.displayName}</strong><small>{account.email}</small><em className={`connection-${account.status}`}>{account.status === 'connected' ? '连接正常' : account.status === 'syncing' ? '正在同步' : account.lastError || '连接异常'}</em></span>
        <div><small>{account.authMethod === 'oauth2' ? 'OAuth 2.0' : '授权码 / 专用密码'}</small><button type="button" className="retry-account" disabled={busyId === account.id} onClick={() => void retryConnection(account)}><ArrowClockwise size={15} />{busyId === account.id ? '正在检查' : '重试连接'}</button>{account.authMethod === 'oauth2' ? <button type="button" className="reconnect-account" disabled={busyId === account.id} onClick={() => void reconnect(account)}><Key size={15} />重新授权</button> : <button type="button" className="reconnect-account" disabled={busyId === account.id} onClick={() => setCredentialId((current) => current === account.id ? null : account.id)}><Key size={15} />更新凭据</button>}<button type="button" className="remove-account" disabled={busyId === account.id} onClick={() => void remove(account)}><Trash size={15} />移除</button></div>
      </article>{credentialId === account.id && <form className="credential-renewal" onSubmit={(event) => void updateCredential(event, account)}><label><span>{credentialGuideFor(account.provider)?.secretLabel || '新的授权码 / 应用专用密码'}</span><input name="password" type="password" placeholder={credentialGuideFor(account.provider)?.secretPlaceholder || '输入新的专用凭据'} autoFocus required /></label><button type="button" onClick={() => setCredentialId(null)}>取消</button><Button appearance="primary" type="submit" disabled={busyId === account.id}>{busyId === account.id ? '正在验证…' : '验证并更新'}</Button></form>}</div>)}
    </div>}
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
    <div className="modal-footer"><Button appearance="primary" onClick={onClose}>完成</Button></div>
  </section></Overlay>;
}


