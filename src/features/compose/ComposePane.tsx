import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowLeft, File, PaperPlaneTilt, Trash, WarningCircle } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Account, Contact, Draft, DraftAttachment, Message } from '../../types';
import { AppInput } from '../../components/form-controls';
import { RichTextEditor } from './RichTextEditor';
import { AddressField, type AddressFieldHandle } from './AddressField';
import { SenderField } from './SenderField';
import { fileAsAttachment, formatAttachmentSize, subjectWithPrefix, textToHtml } from './compose-utils';

export type ComposePaneHandle = { close: () => Promise<void> };

export const ComposePane = forwardRef<ComposePaneHandle, {
  accounts: Account[]; contacts: Contact[]; mode: 'new' | 'reply' | 'forward'; initialAccountId?: string; original?: Message; draft?: Draft;
  onClose: () => void | Promise<void>; onSent: () => void | Promise<void>; onDraftSaved: (draft: Draft) => void;
}>(function ComposePane({ accounts, contacts, mode, initialAccountId, original, draft, onClose, onSent, onDraftSaved }, ref) {
  const isReply = mode === 'reply'; const isForward = mode === 'forward';
  const initialSubject = draft?.subject ?? (original ? subjectWithPrefix(original.subject, isReply ? 'Re' : 'Fwd') : '');
  const quoteText = original ? `\n\n----- ${isForward ? '转发邮件' : '原邮件'} -----\n发件人：${original.from.name || original.from.address} <${original.from.address}>\n${original.text ?? ''}` : '';
  const initialText = draft?.text ?? quoteText;
  const [accountId, setAccountId] = useState(draft?.accountId ?? original?.accountId ?? initialAccountId ?? accounts[0]?.id ?? '');
  const [to, setTo] = useState<string[]>(draft?.to ?? (isReply && original?.from.address ? [original.from.address] : []));
  const [cc, setCc] = useState<string[]>(draft?.cc ?? []);
  const [subject, setSubject] = useState(initialSubject);
  const [html, setHtml] = useState(draft?.html || textToHtml(initialText));
  const [text, setText] = useState(initialText);
  const [attachments, setAttachments] = useState<DraftAttachment[]>(draft?.attachments ?? []);
  const [error, setError] = useState('');
  const [sending, setSending] = useState(false);
  const [saveStatus, setSaveStatus] = useState<'saved' | 'pending' | 'saving' | 'error'>(draft ? 'saved' : 'pending');
  const draftIdRef = useRef(draft?.id ?? crypto.randomUUID());
  const draftCreatedRef = useRef(Boolean(draft));
  const revisionRef = useRef(mode === 'new' && !draft ? 0 : 1);
  const savedRevisionRef = useRef(draft ? revisionRef.current : 0);
  const savingRef = useRef(false);
  const savingPromiseRef = useRef<Promise<void> | null>(null);
  const queuedRef = useRef(false);
  const lastSavedPayloadRef = useRef(draft ? JSON.stringify({ accountId: draft.accountId, to: draft.to, cc: draft.cc, subject: draft.subject, text: draft.text, html: draft.html, attachments: draft.attachments }) : '');
  const [saveTick, setSaveTick] = useState(0);
  const initialHtml = useMemo(() => html, []);
  const toFieldRef = useRef<AddressFieldHandle>(null);
  const ccFieldRef = useRef<AddressFieldHandle>(null);

  const markDirty = () => { revisionRef.current += 1; setSaveStatus('pending'); };
  const persistDraft = useCallback(async (addressOverride?: { to: string[]; cc: string[] }) => {
    if (!accountId || revisionRef.current === savedRevisionRef.current) return;
    if (savingRef.current) { queuedRef.current = true; return savingPromiseRef.current ?? undefined; }
    const savingRevision = revisionRef.current;
    const body = { accountId, to: addressOverride?.to ?? to, cc: addressOverride?.cc ?? cc, subject, text, html, attachments };
    const payload = JSON.stringify(body);
    if (payload === lastSavedPayloadRef.current) {
      savedRevisionRef.current = savingRevision;
      setSaveStatus('saved');
      return;
    }
    savingRef.current = true; setSaveStatus('saving'); setError('');
    const operation = (async () => {
      try {
        const result = await api<{ draft: Draft }>(draftCreatedRef.current ? `/api/drafts/${draftIdRef.current}` : '/api/drafts', {
          method: draftCreatedRef.current ? 'PUT' : 'POST',
          headers: draftCreatedRef.current ? undefined : { 'X-Draft-Id': draftIdRef.current },
          body: payload,
        });
        draftIdRef.current = result.draft.id; draftCreatedRef.current = true; savedRevisionRef.current = savingRevision; lastSavedPayloadRef.current = payload; onDraftSaved(result.draft);
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
    const resolvedTo = toFieldRef.current?.resolve() ?? { addresses: to };
    const resolvedCc = ccFieldRef.current?.resolve() ?? { addresses: cc };
    const invalid = [resolvedTo.invalid, resolvedCc.invalid].filter(Boolean);
    if (invalid.length > 0) { setError(`邮箱地址不完整或格式错误：${invalid.join('、')}`); return; }
    if (resolvedTo.addresses.length === 0) { setError('请填写至少一个收件人'); return; }
    if (!subject.trim()) { setError('请填写邮件主题'); return; }
    if (!text.trim() && !/<img\b/i.test(html)) { setError('请填写邮件正文'); return; }
    setSending(true);
    try { await api('/api/send', { method: 'POST', body: JSON.stringify({ accountId, to: resolvedTo.addresses, cc: resolvedCc.addresses, subject: subject.trim(), text: text.trim() || '邮件包含图片内容', html, attachments, draftId: draftIdRef.current }) }); await onSent(); }
    catch (value) { setError(value instanceof Error ? value.message : '发送失败'); } finally { setSending(false); }
  }

  async function close() {
    const resolvedTo = toFieldRef.current?.resolve() ?? { addresses: to };
    const resolvedCc = ccFieldRef.current?.resolve() ?? { addresses: cc };
    if (savingPromiseRef.current) await savingPromiseRef.current;
    if (revisionRef.current !== savedRevisionRef.current) await persistDraft({ to: resolvedTo.addresses, cc: resolvedCc.addresses });
    await onClose();
  }
  useImperativeHandle(ref, () => ({ close }));
  const heading = draft ? '编辑草稿' : isReply ? '回复邮件' : isForward ? '转发邮件' : '写邮件';
  const statusLabel = saveStatus === 'saving' ? '正在保存…' : saveStatus === 'saved' ? '已自动保存' : saveStatus === 'error' ? '自动保存失败' : '等待自动保存';
  return <article className="composer-pane">
    <header className="composer-header">
      <button className="composer-close" type="button" title="关闭写信" aria-label="关闭写信" onClick={() => void close()}><ArrowLeft size={19} /></button>
      <div className="composer-heading"><span>{mode === 'new' ? '新邮件' : '邮件操作'}</span><strong>{heading}</strong></div>
      <div className="composer-header-actions"><small className={`compose-save-status is-${saveStatus}`}>{statusLabel}</small>{accounts.length > 0 && <Button className="compose-header-send" appearance="primary" icon={<PaperPlaneTilt size={16} />} type="submit" form="compose-message-form" disabled={sending}>{sending ? '发送中…' : '发送'}</Button>}</div>
    </header>
    {accounts.length === 0 ? <div className="compose-empty"><WarningCircle size={34} /><h3>先接入一个真实邮箱</h3><p>接入邮箱后即可发送邮件。</p></div> : <form id="compose-message-form" className="composer-form" onSubmit={submit}>
      <div className="composer-fields">
        <SenderField accounts={accounts} value={accountId} onChange={(value) => { setAccountId(value); markDirty(); }} />
        <AddressField ref={toFieldRef} label="收件人" value={to} contacts={contacts} onChange={(value) => { setTo(value); markDirty(); }} placeholder="输入姓名或邮箱" />
        <AddressField ref={ccFieldRef} label="抄送" value={cc} contacts={contacts} onChange={(value) => { setCc(value); markDirty(); }} placeholder="输入姓名或邮箱（可选）" />
        <label className="compose-row"><span>主题</span><AppInput value={subject} onChange={(event) => { setSubject(event.target.value); markDirty(); }} placeholder="邮件主题" /></label>
      </div>
      <RichTextEditor initialHtml={initialHtml} onChange={(nextHtml, nextText) => { setHtml(nextHtml); setText(nextText); markDirty(); }} onAddAttachments={(files) => void addAttachments(files)} onError={setError} />
      {attachments.length > 0 && <div className="composer-attachments">{attachments.map((attachment) => <span key={attachment.id}><File size={18} weight="duotone" /><span><strong>{attachment.filename}</strong><small>{formatAttachmentSize(attachment.size)}</small></span><button type="button" title={`移除 ${attachment.filename}`} aria-label={`移除附件 ${attachment.filename}`} onClick={() => { setAttachments((current) => current.filter((item) => item.id !== attachment.id)); markDirty(); }}><Trash size={15} /></button></span>)}</div>}
      {error && <div className="inline-error composer-error"><WarningCircle size={17} />{error}</div>}
      <footer className="composer-footer"><span>关闭写信后仍会保留草稿</span></footer>
    </form>}
  </article>;
});
