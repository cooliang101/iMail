import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowLeft, File, PaperPlaneTilt, Trash, WarningCircle } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Account, Contact, Draft, DraftAttachment, Message } from '../../types';
import { providerLabel } from '../../components/shared';
import { AppInput, AppSelect } from '../../components/form-controls';
import { RichTextEditor } from './RichTextEditor';
import { AddressField } from './AddressField';
import { addressParts, invalidAddresses, validAddresses } from './address-utils';

function subjectWithPrefix(subject: string, prefix: 'Re' | 'Fwd') { return new RegExp(`^${prefix}:`, 'i').test(subject) ? subject : `${prefix}: ${subject}`; }
function escapeHtml(value: string) { return value.replace(/[&<>"']/g, (character) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#039;' })[character] ?? character); }
function textToHtml(value: string) { return value.split('\n').map((line) => `<p>${line ? escapeHtml(line) : '<br>'}</p>`).join(''); }
function formatSize(size: number) { return size < 1024 * 1024 ? `${Math.max(1, Math.round(size / 1024))} KB` : `${(size / 1024 / 1024).toFixed(1)} MB`; }
function fileAsAttachment(file: File) {
  return new Promise<DraftAttachment>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve({ id: crypto.randomUUID(), filename: file.name, contentType: file.type || 'application/octet-stream', size: file.size, data: String(reader.result).split(',')[1] ?? '' });
    reader.onerror = () => reject(new Error(`无法读取 ${file.name}`));
    reader.readAsDataURL(file);
  });
}

export type ComposePaneHandle = { close: () => Promise<void> };

export const ComposePane = forwardRef<ComposePaneHandle, {
  accounts: Account[]; contacts: Contact[]; mode: 'new' | 'reply' | 'forward'; original?: Message; draft?: Draft;
  onClose: () => void | Promise<void>; onSent: () => void | Promise<void>; onDraftSaved: (draft: Draft) => void;
}>(function ComposePane({ accounts, contacts, mode, original, draft, onClose, onSent, onDraftSaved }, ref) {
  const isReply = mode === 'reply'; const isForward = mode === 'forward';
  const initialSubject = draft?.subject ?? (original ? subjectWithPrefix(original.subject, isReply ? 'Re' : 'Fwd') : '');
  const quoteText = original ? `\n\n----- ${isForward ? '转发邮件' : '原邮件'} -----\n发件人：${original.from.name || original.from.address} <${original.from.address}>\n${original.text ?? ''}` : '';
  const initialText = draft?.text ?? quoteText;
  const [accountId, setAccountId] = useState(draft?.accountId ?? original?.accountId ?? accounts[0]?.id ?? '');
  const [to, setTo] = useState(draft?.to.join(', ') ?? (isReply ? original?.from.address ?? '' : ''));
  const [cc, setCc] = useState(draft?.cc.join(', ') ?? '');
  const [subject, setSubject] = useState(initialSubject);
  const [html, setHtml] = useState(draft?.html || textToHtml(initialText));
  const [text, setText] = useState(initialText);
  const [attachments, setAttachments] = useState<DraftAttachment[]>(draft?.attachments ?? []);
  const [error, setError] = useState('');
  const [sending, setSending] = useState(false);
  const [saveStatus, setSaveStatus] = useState<'saved' | 'pending' | 'saving' | 'error'>(draft ? 'saved' : 'pending');
  const draftIdRef = useRef(draft?.id);
  const revisionRef = useRef(mode === 'new' && !draft ? 0 : 1);
  const savedRevisionRef = useRef(draft ? revisionRef.current : 0);
  const savingRef = useRef(false);
  const savingPromiseRef = useRef<Promise<void> | null>(null);
  const queuedRef = useRef(false);
  const [saveTick, setSaveTick] = useState(0);
  const initialHtml = useMemo(() => html, []);

  const markDirty = () => { revisionRef.current += 1; setSaveStatus('pending'); };
  const persistDraft = useCallback(async () => {
    if (!accountId || revisionRef.current === savedRevisionRef.current) return;
    if (savingRef.current) { queuedRef.current = true; return savingPromiseRef.current ?? undefined; }
    savingRef.current = true; const savingRevision = revisionRef.current; setSaveStatus('saving'); setError('');
    const body = { accountId, to: addressParts(to), cc: addressParts(cc), subject, text, html, attachments };
    const operation = (async () => {
      try {
        const result = await api<{ draft: Draft }>(draftIdRef.current ? `/api/drafts/${draftIdRef.current}` : '/api/drafts', { method: draftIdRef.current ? 'PUT' : 'POST', body: JSON.stringify(body) });
        draftIdRef.current = result.draft.id; savedRevisionRef.current = savingRevision; onDraftSaved(result.draft);
        setSaveStatus(revisionRef.current === savingRevision ? 'saved' : 'pending');
      } catch (value) { setSaveStatus('error'); setError(value instanceof Error ? value.message : '草稿自动保存失败'); }
      finally { savingRef.current = false; savingPromiseRef.current = null; if (queuedRef.current) { queuedRef.current = false; setSaveTick((value) => value + 1); } }
    })();
    savingPromiseRef.current = operation;
    await operation;
  }, [accountId, attachments, cc, html, onDraftSaved, subject, text, to]);

  useEffect(() => {
    if (revisionRef.current === savedRevisionRef.current || !accountId) return;
    const timer = window.setTimeout(() => void persistDraft(), 900);
    return () => window.clearTimeout(timer);
  }, [accountId, attachments, cc, html, persistDraft, saveTick, subject, text, to]);

  async function addAttachments(files: File[]) {
    const accepted = files.filter((file) => file.size <= 5 * 1024 * 1024);
    if (accepted.length !== files.length) setError('已忽略超过 5 MB 的附件');
    if (attachments.length + accepted.length > 10) { setError('最多添加 10 个附件'); return; }
    if (attachments.reduce((total, item) => total + item.size, 0) + accepted.reduce((total, file) => total + file.size, 0) > 15 * 1024 * 1024) { setError('附件总大小不能超过 15 MB'); return; }
    try { const next = await Promise.all(accepted.map(fileAsAttachment)); setAttachments((current) => [...current, ...next]); markDirty(); }
    catch (value) { setError(value instanceof Error ? value.message : '附件读取失败'); }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setError('');
    const invalid = [...invalidAddresses(to), ...invalidAddresses(cc)];
    if (invalid.length > 0) { setError(`邮箱地址不完整或格式错误：${invalid.join('、')}`); return; }
    if (validAddresses(to).length === 0) { setError('请填写至少一个收件人'); return; }
    if (!subject.trim()) { setError('请填写邮件主题'); return; }
    if (!text.trim() && !/<img\b/i.test(html)) { setError('请填写邮件正文'); return; }
    setSending(true);
    try { await api('/api/send', { method: 'POST', body: JSON.stringify({ accountId, to: validAddresses(to), cc: validAddresses(cc), subject: subject.trim(), text: text.trim() || '邮件包含图片内容', html, attachments, draftId: draftIdRef.current }) }); await onSent(); }
    catch (value) { setError(value instanceof Error ? value.message : '发送失败'); } finally { setSending(false); }
  }

  async function close() { if (savingPromiseRef.current) await savingPromiseRef.current; if (revisionRef.current !== savedRevisionRef.current) await persistDraft(); await onClose(); }
  useImperativeHandle(ref, () => ({ close }));
  const heading = draft ? '编辑草稿' : isReply ? '回复邮件' : isForward ? '转发邮件' : '写邮件';
  const statusLabel = saveStatus === 'saving' ? '正在保存…' : saveStatus === 'saved' ? '已自动保存' : saveStatus === 'error' ? '自动保存失败' : '等待自动保存';
  return <article className="composer-pane">
    <header className="composer-header"><button type="button" title="关闭写信" aria-label="关闭写信" onClick={() => void close()}><ArrowLeft size={19} /></button><div><span>{mode === 'new' ? '新邮件' : '邮件操作'}</span><strong>{heading}</strong></div><small className={`compose-save-status is-${saveStatus}`}>{statusLabel}</small></header>
    {accounts.length === 0 ? <div className="compose-empty"><WarningCircle size={34} /><h3>先接入一个真实邮箱</h3><p>接入邮箱后即可发送邮件。</p></div> : <form className="composer-form" onSubmit={submit}>
      <div className="composer-fields">
        <label className="compose-row"><span>发件人</span><AppSelect value={accountId} onValueChange={(value) => { setAccountId(value); markDirty(); }} options={accounts.map((account) => ({ value: account.id, label: `${providerLabel[account.provider]} · ${account.displayName} · ${account.email}` }))} /></label>
        <AddressField label="收件人" value={to} contacts={contacts} onChange={(value) => { setTo(value); markDirty(); }} placeholder="输入姓名、邮箱或 @ 查找联系人" />
        <AddressField label="抄送" value={cc} contacts={contacts} onChange={(value) => { setCc(value); markDirty(); }} placeholder="可选；输入 @ 快速选择" />
        <label className="compose-row"><span>主题</span><AppInput value={subject} onChange={(event) => { setSubject(event.target.value); markDirty(); }} placeholder="邮件主题" /></label>
      </div>
      <RichTextEditor initialHtml={initialHtml} onChange={(nextHtml, nextText) => { setHtml(nextHtml); setText(nextText); markDirty(); }} onAddAttachments={(files) => void addAttachments(files)} onError={setError} />
      {attachments.length > 0 && <div className="composer-attachments">{attachments.map((attachment) => <span key={attachment.id}><File size={18} weight="duotone" /><span><strong>{attachment.filename}</strong><small>{formatSize(attachment.size)}</small></span><button type="button" title={`移除 ${attachment.filename}`} aria-label={`移除附件 ${attachment.filename}`} onClick={() => { setAttachments((current) => current.filter((item) => item.id !== attachment.id)); markDirty(); }}><Trash size={15} /></button></span>)}</div>}
      {error && <div className="inline-error composer-error"><WarningCircle size={17} />{error}</div>}
      <footer className="composer-footer"><span>关闭写信后仍会保留草稿</span><Button appearance="primary" icon={<PaperPlaneTilt size={17} />} type="submit" disabled={sending}>{sending ? '发送中…' : '发送邮件'}</Button></footer>
    </form>}
  </article>;
});
