import { useRef, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { PaperPlaneTilt, PencilSimple, Trash, WarningCircle, X } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Account, Draft, Message } from '../../types';
import { AccountProviderMark, Overlay, providerLabel, relativeTime } from '../../components/shared';
import { AppInput, AppSelect, AppTextarea } from '../../components/form-controls';

function subjectWithPrefix(subject: string, prefix: 'Re' | 'Fwd') {
  return new RegExp(`^${prefix}:`, 'i').test(subject) ? subject : `${prefix}: ${subject}`;
}

export function ComposeModal({ accounts, mode, original, draft, onClose, onSent, onSaved }: { accounts: Account[]; mode: 'new' | 'reply' | 'forward'; original?: Message; draft?: Draft; onClose: () => void; onSent: () => void | Promise<void>; onSaved: () => void | Promise<void> }) {
  const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const formRef = useRef<HTMLFormElement | null>(null);
  const isReply = mode === 'reply'; const isForward = mode === 'forward';
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); const form = new FormData(event.currentTarget); setBusy(true); setError('');
    try { await api('/api/send', { method: 'POST', body: JSON.stringify({ accountId: form.get('accountId'), to: String(form.get('to')).split(',').map((item) => item.trim()).filter(Boolean), subject: form.get('subject'), text: form.get('text'), draftId: draft?.id }) }); await onSent(); }
    catch (value) { setError(value instanceof Error ? value.message : '发送失败'); } finally { setBusy(false); }
  }
  async function saveDraft() {
    if (!formRef.current || accounts.length === 0) return;
    const form = new FormData(formRef.current); setBusy(true); setError('');
    const body = { accountId: form.get('accountId'), to: String(form.get('to')).split(',').map((item) => item.trim()).filter(Boolean), cc: [], subject: String(form.get('subject') ?? ''), text: String(form.get('text') ?? '') };
    try { await api(draft ? `/api/drafts/${draft.id}` : '/api/drafts', { method: draft ? 'PUT' : 'POST', body: JSON.stringify(body) }); await onSaved(); }
    catch (value) { setError(value instanceof Error ? value.message : '草稿保存失败'); } finally { setBusy(false); }
  }
  const heading = isReply ? '回复邮件' : isForward ? '转发邮件' : '写邮件';
  const subject = original ? subjectWithPrefix(original.subject, isReply ? 'Re' : 'Fwd') : '';
  const quoted = original ? `\n\n----- ${isForward ? '转发邮件' : '原邮件'} -----\n发件人：${original.from.name || original.from.address} <${original.from.address}>\n${original.text ?? ''}` : '';
  return <Overlay onClose={onClose}><form ref={formRef} className="compose-modal" onSubmit={submit}><div className="modal-header compact"><div><span>{draft ? '本地草稿' : mode === 'new' ? '新邮件' : '邮件操作'}</span><h2>{draft ? '编辑草稿' : heading}</h2></div><button type="button" aria-label="关闭写信窗口" onClick={onClose}><X size={21} /></button></div>
    {accounts.length === 0 ? <div className="compose-empty"><WarningCircle size={30} /><h3>先接入一个真实邮箱</h3><p>接入邮箱后即可发送邮件。</p></div> : <><label className="compose-row"><span>发件人</span><AppSelect name="accountId" defaultValue={draft?.accountId ?? original?.accountId ?? accounts[0]?.id} options={accounts.map((account) => ({ value: account.id, label: `${providerLabel[account.provider]} · ${account.displayName} · ${account.email}` }))} /></label><label className="compose-row"><span>收件人</span><AppInput name="to" type="email" multiple defaultValue={draft?.to.join(', ') ?? (isReply ? original?.from.address : '')} placeholder="多个地址用英文逗号分隔" required /></label><label className="compose-row"><span>主题</span><AppInput name="subject" defaultValue={draft?.subject ?? subject} required /></label><AppTextarea name="text" className="compose-body" defaultValue={draft?.text ?? quoted} placeholder="写下邮件内容…" required />{error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}<div className="modal-footer"><button type="button" onClick={onClose}>取消</button><button type="button" disabled={busy} onClick={() => void saveDraft()}>{busy ? '保存中…' : '存为草稿'}</button><Button appearance="primary" icon={<PaperPlaneTilt size={17} />} type="submit" disabled={busy}>{busy ? '处理中…' : '发送邮件'}</Button></div></>}
  </form></Overlay>;
}

