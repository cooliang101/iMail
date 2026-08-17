import '../../styles/dialogs.css';
import { useState, type FormEvent } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { Check, WarningCircle, X } from '../../components/icons';
import { api } from '../../api';
import type { Account } from '../../types';
import { Overlay, ProviderIcon, providerLabel } from '../../components/shared';
import { AppCheckbox, AppInput, AppSelect } from '../../components/form-controls';
import { TokenCreatedResult } from './TokenCreatedResult';

export function CreateApiTokenModal({ accounts, onClose, onCreated }: { accounts: Account[]; onClose: () => void; onCreated: () => Promise<void> }) {
  const [raw, setRaw] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('');
    const form = new FormData(event.currentTarget);
    const mailboxes = form.getAll('mailboxes');
    const scopes = form.getAll('scopes');
    if (mailboxes.length === 0) { setError('请至少选择一个允许访问的邮箱'); setBusy(false); return; }
    if (scopes.length === 0) { setError('请至少选择一项 API 权限'); setBusy(false); return; }
    try {
      const result = await api<{ token: string }>('/api/developer-tokens', { method: 'POST', body: JSON.stringify({ name: form.get('name'), mailboxes, scopes, ttlSeconds: Number(form.get('ttlSeconds')) }) });
      setRaw(result.token); await onCreated();
    } catch (value) { setError(value instanceof Error ? value.message : 'API Token 创建失败'); }
    finally { setBusy(false); }
  }

  return <Overlay onClose={onClose}>{raw ? <TokenCreatedResult raw={raw} kind="API Token" onClose={onClose} /> : <form className="token-modal" onSubmit={submit}>
    <div className="modal-header"><div><span>邮件 API 网关</span><h2>创建 API Token</h2><p>限制可访问邮箱和具体 API 能力。</p></div><button type="button" aria-label="关闭 API Token 创建窗口" onClick={onClose}><X size={21} /></button></div>
    <label><span>用途名称</span><AppInput name="name" defaultValue="本地 API 调用" required /></label>
    <fieldset><legend>允许访问的邮箱</legend>{accounts.map((account) => <label className="check-row" key={account.id}><AppCheckbox name="mailboxes" value={account.email} defaultChecked /><i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i><span><strong>{providerLabel[account.provider]} · {account.displayName}</strong><small>{account.email}</small></span><Check size={15} /></label>)}</fieldset>
    <fieldset><legend>API 权限</legend><label className="scope-row"><AppCheckbox name="scopes" value="messages:read" defaultChecked /><span><strong>读取邮件</strong><small>获取正文、发件人与附件元数据</small></span></label><label className="scope-row"><AppCheckbox name="scopes" value="messages:send" /><span><strong>发送邮件</strong><small>通过选定邮箱发送新邮件</small></span></label><label className="scope-row"><AppCheckbox name="scopes" value="accounts:read" /><span><strong>读取账户</strong><small>获取邮箱列表和连接状态</small></span></label></fieldset>
    <label><span>有效时间</span><AppSelect name="ttlSeconds" defaultValue="3600" options={[{ value: '1800', label: '30 分钟' }, { value: '3600', label: '1 小时' }, { value: '21600', label: '6 小时' }, { value: '86400', label: '24 小时' }, { value: '604800', label: '7 天' }]} /></label>
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
    <div className="modal-footer"><button type="button" onClick={onClose}>取消</button><AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '创建中…' : '创建 API Token'}</AppButton></div>
  </form>}</Overlay>;
}
