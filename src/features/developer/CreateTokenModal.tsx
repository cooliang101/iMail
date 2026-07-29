import { useEffect, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { AddressBook, ArrowRight, Check, Code, Copy, Key, Plus, WarningCircle, X } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Account, DeveloperToken } from '../../types';
import type { Notice } from '../../app-model';
import { Overlay, ProviderIcon, providerLabel } from '../../components/shared';

export function CreateTokenModal({ accounts, onClose, onCreated }: { accounts: Account[]; onClose: () => void; onCreated: () => Promise<void> }) {
  const [raw, setRaw] = useState(''); const [busy, setBusy] = useState(false); const [copied, setCopied] = useState(false); const [error, setError] = useState('');
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('');
    const form = new FormData(event.currentTarget); const accountIds = form.getAll('accountIds'); const scopes = form.getAll('scopes');
    if (accountIds.length === 0) { setError('请至少选择一个允许访问的邮箱'); setBusy(false); return; }
    if (scopes.length === 0) { setError('请至少选择一项权限'); setBusy(false); return; }
    try { const result = await api<{ token: string }>('/api/developer-tokens', { method: 'POST', body: JSON.stringify({ name: form.get('name'), accountIds, scopes, ttlSeconds: Number(form.get('ttlSeconds')) }) }); setRaw(result.token); await onCreated(); }
    catch (value) { setError(value instanceof Error ? value.message : 'Token 创建失败'); }
    finally { setBusy(false); }
  }
  return <Overlay onClose={onClose}>{raw ? <div className="token-created"><div className="success-orbit"><Check size={28} weight="bold" /></div><h2>Token 已创建</h2><p>请现在复制并保存，关闭后无法再次查看完整值。</p><div className="raw-token"><code>{raw}</code><button onClick={async () => { try { await navigator.clipboard.writeText(raw); setCopied(true); } catch { setError('复制失败，请手动选择 Token'); } }}><Copy size={17} />{copied ? '已复制' : '复制'}</button></div>{error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}<Button appearance="primary" onClick={onClose}>完成</Button></div> : <form className="token-modal" onSubmit={submit}><div className="modal-header"><div><span>开发者网关</span><h2>创建临时 Token</h2><p>控制可访问邮箱、能力和有效时间。</p></div><button type="button" aria-label="关闭 Token 创建窗口" onClick={onClose}><X size={21} /></button></div><label><span>用途名称</span><input name="name" defaultValue="本地开发测试" required /></label><fieldset><legend>允许访问的邮箱</legend>{accounts.map((account) => <label className="check-row" key={account.id}><input name="accountIds" type="checkbox" value={account.id} defaultChecked /><i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i><span><strong>{providerLabel[account.provider]} · {account.displayName}</strong><small>{account.email}</small></span><Check size={15} /></label>)}</fieldset><fieldset><legend>权限范围</legend><label className="scope-row"><input name="scopes" type="checkbox" value="messages:read" defaultChecked /><span><strong>读取邮件</strong><small>获取正文、发件人与附件元数据</small></span></label><label className="scope-row"><input name="scopes" type="checkbox" value="messages:send" /><span><strong>发送邮件</strong><small>通过选定邮箱发送新邮件</small></span></label><label className="scope-row"><input name="scopes" type="checkbox" value="accounts:read" /><span><strong>读取账户</strong><small>获取邮箱列表和连接状态</small></span></label></fieldset><label><span>有效时间</span><select name="ttlSeconds" defaultValue="3600"><option value="1800">30 分钟</option><option value="3600">1 小时</option><option value="21600">6 小时</option><option value="86400">24 小时</option><option value="604800">7 天</option></select></label>{error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}<div className="modal-footer"><button type="button" onClick={onClose}>取消</button><Button appearance="primary" type="submit" disabled={busy}>{busy ? '创建中…' : '创建 Token'}</Button></div></form>}</Overlay>;
}


