import { useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowRight, Bell, Check, Clock, Envelope, Tag, WarningCircle, X } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Account, Message } from '../../types';
import type { MailNotification } from '../../app-model';
import { Overlay, ProviderIcon, providerLabel, relativeTime } from '../../components/shared';

export function WorkspaceModal({ accounts, onClose, onSaved }: { accounts: Account[]; onClose: () => void; onSaved: () => void | Promise<void> }) {
  const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); const form = new FormData(event.currentTarget); const group = String(form.get('group')).trim(); const accountIds = form.getAll('accountIds').map(String);
    if (accountIds.length === 0) { setError('请至少选择一个邮箱'); return; }
    setBusy(true); setError('');
    try { await Promise.all(accountIds.map((id) => api(`/api/accounts/${id}`, { method: 'PATCH', body: JSON.stringify({ group }) }))); await onSaved(); }
    catch (value) { setError(value instanceof Error ? value.message : '工作空间保存失败'); } finally { setBusy(false); }
  }
  return <Overlay onClose={onClose}><form className="utility-modal workspace-modal" onSubmit={submit}><div className="modal-header"><div><span>集中整理</span><h2>新增工作空间</h2><p>给一组邮箱设置相同的工作空间名称。</p></div><button type="button" aria-label="关闭工作空间窗口" onClick={onClose}><X size={21} /></button></div><label className="workspace-name"><span>工作空间名称</span><input name="group" placeholder="例如：客户支持、开发测试" maxLength={40} required autoFocus /></label><fieldset><legend>包含的邮箱</legend>{accounts.map((account) => <label className="check-row" key={account.id}><input name="accountIds" type="checkbox" value={account.id} /><i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i><span><strong>{providerLabel[account.provider]} · {account.displayName}</strong><small>{account.email} · 当前：{account.group}</small></span><Check size={15} /></label>)}</fieldset>{error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}<div className="modal-footer"><button type="button" onClick={onClose}>取消</button><Button appearance="primary" type="submit" disabled={busy}>{busy ? '保存中…' : '保存工作空间'}</Button></div></form></Overlay>;
}


