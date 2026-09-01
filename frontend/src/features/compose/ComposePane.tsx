import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState, type FormEvent } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { ArrowLeft, Clock, File, PaperPlaneTilt, Trash, WarningCircle } from '../../components/icons';
import { api } from '../../services';
import type { Account, Contact, Draft, DraftAttachment, Message } from '../../types';
import type { CompositionPreferences, ComposeMode } from '../../app-model';
import { replyHeaders, replyRecipients, needsAttachmentReminder } from './reply-model';
import { signatureHtml } from './signature-node';
import { AppInput, AppSelect } from '../../components/form-controls';
import { RichTextEditor, type RichTextEditorHandle } from './RichTextEditor';
import { AddressField, type AddressFieldHandle } from './AddressField';
import { SenderField } from './SenderField';
import { fileAsAttachment, formatAttachmentSize, subjectWithPrefix, textToHtml } from './compose-utils';
import { dateTimeLocalValue, nextHourLocalValue } from './schedule-send';

export type ComposePaneHandle = { close: () => Promise<boolean> };

export const ComposePane = forwardRef<ComposePaneHandle, {
  accounts: Account[]; contacts: Contact[]; mode: ComposeMode; composition?: CompositionPreferences; initialAccountId?: string; initialTo?: string[]; original?: Message; draft?: Draft;
  onClose: () => void | Promise<void>; onSent: () => void | Promise<void>; onScheduled: () => void | Promise<void>; onDraftSaved: (draft: Draft) => void;
}>(function ComposePane({ accounts, contacts, mode, composition, initialAccountId, initialTo, original, draft, onClose, onSent, onScheduled, onDraftSaved }, ref) {
  const isReply = mode === 'reply' || mode === 'replyAll'; const isForward = mode === 'forward';
  const initialSubject = draft?.subject ?? (original ? subjectWithPrefix(original.subject, isReply ? 'Re' : 'Fwd') : '');
  const quoteText = original ? `\n\n----- ${isForward ? '转发邮件' : '原邮件'} -----\n发件人：${original.from.name || original.from.address} <${original.from.address}>\n${original.text ?? ''}` : '';
  const initialSenderId = draft?.accountId ?? original?.accountId ?? initialAccountId ?? accounts[0]?.id ?? '';
  const signatureFor = (senderId: string) => {
    const value = composition?.signatures.find(item => item.accountId === senderId);
    return value && (isReply ? value.replies : value.newMessages) ? value.text : '';
  };
  const initialSignature = draft ? '' : signatureFor(initialSenderId);
  const initialText = draft?.text ?? [initialSignature, quoteText].filter(Boolean).join('\n');
  const recipients = replyRecipients(original, accounts, mode === 'replyAll');
  const envelope = useRef(draft ? { inReplyTo: draft.inReplyTo ?? [], references: draft.references ?? [] } : isReply ? replyHeaders(original) : { inReplyTo: [], references: [] }).current;
  const [accountId, setAccountId] = useState(initialSenderId);
  const [to, setTo] = useState<string[]>(draft?.to ?? (isReply ? recipients.to : initialTo ?? []));
  const [cc, setCc] = useState<string[]>(draft?.cc ?? (isReply ? recipients.cc : []));
  const [bcc, setBcc] = useState<string[]>(draft?.bcc ?? []);
  const [attachmentWarning, setAttachmentWarning] = useState(false);
  const [signatureNotice, setSignatureNotice] = useState('');
  const editorRef = useRef<RichTextEditorHandle>(null);
  const [subject, setSubject] = useState(initialSubject);
  const [html, setHtml] = useState(draft?.html || (draft ? textToHtml(initialText) : `<p><br></p>${signatureHtml(initialSenderId, initialSignature)}${quoteText ? `<blockquote>${textToHtml(quoteText.trim())}</blockquote>` : ''}`));
  const [text, setText] = useState(initialText);
  const [attachments, setAttachments] = useState<DraftAttachment[]>(draft?.attachments ?? []);
  const [error, setError] = useState('');
  const [sending, setSending] = useState(false);
  const [scheduleOpen, setScheduleOpen] = useState(false);
  const [customSendAt, setCustomSendAt] = useState(() => nextHourLocalValue());
  const [pendingSendAt, setPendingSendAt] = useState<string>();
  const [saveStatus, setSaveStatus] = useState<'saved' | 'pending' | 'saving' | 'error'>(draft ? 'saved' : 'pending');
  const draftIdRef = useRef(draft?.id ?? crypto.randomUUID());
  const scheduleRequestIdRef = useRef(crypto.randomUUID());
  const draftCreatedRef = useRef(Boolean(draft));
  const revisionRef = useRef(mode === 'new' && !draft ? 0 : 1);
  const savedRevisionRef = useRef(draft ? revisionRef.current : 0);
  const savingRef = useRef(false);
  const savingPromiseRef = useRef<Promise<void> | null>(null);
  const queuedRef = useRef(false);
  const lastSavedPayloadRef = useRef(draft ? JSON.stringify({ accountId: draft.accountId, to: draft.to, cc: draft.cc, bcc: draft.bcc ?? [], ...envelope, subject: draft.subject, text: draft.text, html: draft.html, attachments: draft.attachments }) : '');
  const [saveTick, setSaveTick] = useState(0);
  const initialHtml = useRef(html).current;
  const toFieldRef = useRef<AddressFieldHandle>(null);
  const ccFieldRef = useRef<AddressFieldHandle>(null);
  const bccFieldRef = useRef<AddressFieldHandle>(null);
  const sendingRef = useRef(false);

  const markDirty = () => { setAttachmentWarning(false); revisionRef.current += 1; setSaveStatus('pending'); };
  const persistDraft = useCallback(async (addressOverride?: { to: string[]; cc: string[]; bcc: string[] }) => {
    if (!accountId || revisionRef.current === savedRevisionRef.current) return;
    if (savingRef.current) { queuedRef.current = true; return savingPromiseRef.current ?? undefined; }
    const savingRevision = revisionRef.current;
    const body = { accountId, to: addressOverride?.to ?? to, cc: addressOverride?.cc ?? cc, bcc: addressOverride?.bcc ?? bcc, ...envelope, subject, text, html, attachments };
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
  }, [accountId, attachments, bcc, cc, envelope, html, onDraftSaved, subject, text, to]);

  useEffect(() => {
    if (sending || revisionRef.current === savedRevisionRef.current || !accountId) return;
    const timer = window.setTimeout(() => void persistDraft(), 900);
    return () => window.clearTimeout(timer);
  }, [accountId, attachments, bcc, cc, html, persistDraft, saveTick, sending, subject, text, to]);

  async function addAttachments(files: File[]) {
    const accepted = files.filter((file) => file.size <= 5 * 1024 * 1024);
    if (accepted.length !== files.length) setError('已忽略超过 5 MB 的附件');
    if (attachments.length + accepted.length > 10) { setError('最多添加 10 个附件'); return; }
    if (attachments.reduce((total, item) => total + item.size, 0) + accepted.reduce((total, file) => total + file.size, 0) > 15 * 1024 * 1024) { setError('附件总大小不能超过 15 MB'); return; }
    try { const next = await Promise.all(accepted.map(fileAsAttachment)); setAttachments((current) => [...current, ...next]); markDirty(); }
    catch (value) { setError(value instanceof Error ? value.message : '附件读取失败'); }
  }

  async function submit(event?: FormEvent<HTMLFormElement>, confirmMissingAttachment = false, sendAt?: string) {
    event?.preventDefault();
    if (sendingRef.current) return;
    setError('');
    const resolvedTo = toFieldRef.current?.resolve() ?? { addresses: to };
    const resolvedCc = ccFieldRef.current?.resolve() ?? { addresses: cc };
    const resolvedBcc = bccFieldRef.current?.resolve() ?? { addresses: bcc };
    const invalid = [resolvedTo.invalid, resolvedCc.invalid, resolvedBcc.invalid].filter(Boolean);
    if (invalid.length > 0) { setError(`邮箱地址不完整或格式错误：${invalid.join('、')}`); return; }
    if (resolvedTo.addresses.length + resolvedCc.addresses.length + resolvedBcc.addresses.length === 0) { setError('请填写至少一个收件人'); return; }
    if (!subject.trim()) { setError('请填写邮件主题'); return; }
    if (!text.trim() && !/<img\b/i.test(html)) { setError('请填写邮件正文'); return; }
    if (!confirmMissingAttachment && needsAttachmentReminder(html, attachments.length)) { setPendingSendAt(sendAt); setAttachmentWarning(true); return; }
    setPendingSendAt(undefined); setAttachmentWarning(false); sendingRef.current = true; setSending(true);
    try {
      if (savingPromiseRef.current) await savingPromiseRef.current;
      await persistDraft({ to: resolvedTo.addresses, cc: resolvedCc.addresses, bcc: resolvedBcc.addresses });
      if (!draftCreatedRef.current) throw new Error('草稿保存失败，请重试后发送');
      const payload = { accountId, to: resolvedTo.addresses, cc: resolvedCc.addresses, bcc: resolvedBcc.addresses, ...envelope, subject: subject.trim(), text: text.trim() || '邮件包含图片内容', html, attachments, draftId: draftIdRef.current };
      if (sendAt) {
        await api('/api/outbox', { method: 'POST', body: JSON.stringify({ ...payload, requestId: scheduleRequestIdRef.current, sendAt }) });
        setScheduleOpen(false);
        await onScheduled();
      } else {
        await api('/api/send', { method: 'POST', body: JSON.stringify(payload) });
        await onSent();
      } }
    catch (value) { setError(value instanceof Error ? value.message : '发送失败'); } finally { sendingRef.current = false; setSending(false); }
  }

  async function close() {
    if (sendingRef.current) return false;
    const resolvedTo = toFieldRef.current?.resolve() ?? { addresses: to };
    const resolvedCc = ccFieldRef.current?.resolve() ?? { addresses: cc };
    const resolvedBcc = bccFieldRef.current?.resolve() ?? { addresses: bcc };
    if ([resolvedTo.invalid, resolvedCc.invalid, resolvedBcc.invalid].some(Boolean)) { setError('请先修正未完成的邮箱地址，再关闭写信'); return false; }
    if (savingPromiseRef.current) await savingPromiseRef.current;
    if (revisionRef.current !== savedRevisionRef.current) await persistDraft({ to: resolvedTo.addresses, cc: resolvedCc.addresses, bcc: resolvedBcc.addresses });
    if (revisionRef.current !== savedRevisionRef.current) return false;
    await onClose();
    return true;
  }
  useImperativeHandle(ref, () => ({ close }));
  const heading = draft ? '编辑草稿' : mode === 'replyAll' ? '回复全部' : isReply ? '回复邮件' : isForward ? '转发邮件' : '写邮件';
  const statusLabel = saveStatus === 'saving' ? '正在保存…' : saveStatus === 'saved' ? '已自动保存' : saveStatus === 'error' ? '自动保存失败' : '等待自动保存';
  return <article className="composer-pane">
    <header className="composer-header">
      <button className="composer-close" type="button" title="关闭写信" aria-label="关闭写信" onClick={() => void close()}><ArrowLeft size={19} /></button>
      <div className="composer-heading"><span>{mode === 'new' ? '新邮件' : '邮件操作'}</span><strong>{heading}</strong></div>
      <div className="composer-header-actions"><small className={`compose-save-status is-${saveStatus}`}>{statusLabel}</small>{accounts.length > 0 && <><div className="compose-schedule-anchor"><AppButton className="compose-header-schedule" appearance="subtle" icon={<Clock size={16} />} type="button" aria-label="定时发送" aria-expanded={scheduleOpen} onClick={() => setScheduleOpen((value) => !value)} disabled={sending}>定时</AppButton>{scheduleOpen && <section className="compose-schedule-popover" role="dialog" aria-label="选择发送时间"><header><strong>定时发送</strong><small>按当前设备时区选择</small></header><div className="compose-schedule-quick"><button type="button" onClick={() => void submit(undefined, false, new Date(Date.now() + 10 * 60 * 1000).toISOString())}>10 分钟后</button><button type="button" onClick={() => void submit(undefined, false, new Date(Date.now() + 60 * 60 * 1000).toISOString())}>1 小时后</button><button type="button" onClick={() => { const value = new Date(); value.setDate(value.getDate() + 1); value.setHours(9, 0, 0, 0); void submit(undefined, false, value.toISOString()); }}>明天 09:00</button></div><label><span>自定义时间</span><AppInput type="datetime-local" value={customSendAt} min={dateTimeLocalValue(new Date())} onChange={(event) => setCustomSendAt(event.currentTarget.value)} /></label><AppButton appearance="primary" type="button" disabled={!customSendAt} onClick={() => { const value = new Date(customSendAt); if (Number.isNaN(value.getTime())) { setError('请选择有效的发送时间'); return; } void submit(undefined, false, value.toISOString()); }}>加入发件箱</AppButton><p>任务依赖当前 iMail 服务保持运行。</p></section>}</div><AppButton className="compose-header-send" appearance="primary" icon={<PaperPlaneTilt size={16} />} type="submit" form="compose-message-form" disabled={sending}>{sending ? '处理中…' : '发送'}</AppButton></>}</div>
    </header>
    {accounts.length === 0 ? <div className="compose-empty"><WarningCircle size={34} /><h3>先接入一个真实邮箱</h3><p>接入邮箱后即可发送邮件。</p></div> : <form id="compose-message-form" className="composer-form" inert={sending} aria-busy={sending} onSubmit={submit}>
      <div className="composer-fields">
        <SenderField accounts={accounts} value={accountId} onChange={(value) => { setAccountId(value); if (!editorRef.current?.switchSignature(value, signatureFor(value))) setSignatureNotice('已保留现有正文和签名，请核对发件身份。'); else setSignatureNotice(''); markDirty(); }} />
        <AddressField ref={toFieldRef} label="收件人" value={to} contacts={contacts} onChange={(value) => { setTo(value); markDirty(); }} placeholder="输入姓名或邮箱" />
        <AddressField ref={ccFieldRef} label="抄送" value={cc} contacts={contacts} onChange={(value) => { setCc(value); markDirty(); }} placeholder="输入姓名或邮箱（可选）" />
        <AddressField ref={bccFieldRef} label="密送" value={bcc} contacts={contacts} onChange={(value) => { setBcc(value); markDirty(); }} placeholder="收件人之间不可见（可选）" />
        <label className="compose-row"><span>主题</span><AppInput value={subject} onChange={(event) => { setSubject(event.currentTarget.value); markDirty(); }} placeholder="邮件主题" /></label>
      {!!composition?.templates.length && <label className="compose-row"><span>模板</span><AppSelect aria-label="插入写信模板" value="" options={[{ value: '', label: '选择模板插入正文' }, ...composition.templates.map(template => ({ value: template.id, label: template.name }))]} onValueChange={(id) => { const template = composition.templates.find(item => item.id === id); if (template) { editorRef.current?.insertText(template.text); if (!subject.trim()) setSubject(template.subject); markDirty(); } }} /></label>}
      {signatureNotice && <p role="status">{signatureNotice}</p>}
      </div>
      <RichTextEditor ref={editorRef} manageSignature={!draft} initialHtml={initialHtml} onChange={(nextHtml, nextText) => { setHtml(nextHtml); setText(nextText); markDirty(); }} onAddAttachments={(files) => void addAttachments(files)} onError={setError} />
      {attachments.length > 0 && <div className="composer-attachments">{attachments.map((attachment) => <span key={attachment.id}><File size={18} weight="duotone" /><span><strong>{attachment.filename}</strong><small>{formatAttachmentSize(attachment.size)}</small></span><button type="button" title={`移除 ${attachment.filename}`} aria-label={`移除附件 ${attachment.filename}`} onClick={() => { setAttachments((current) => current.filter((item) => item.id !== attachment.id)); markDirty(); }}><Trash size={15} /></button></span>)}</div>}
      {attachmentWarning && <div className="composer-error" role="alert"><p>正文提到了附件，但尚未添加附件。</p><AppButton type="button" onClick={() => { setAttachmentWarning(false); setPendingSendAt(undefined); }}>返回添加附件</AppButton><AppButton type="button" disabled={sending} onClick={() => void submit(undefined, true, pendingSendAt)}>{pendingSendAt ? '仍然定时发送' : '仍然发送'}</AppButton></div>}
      {error && <div className="inline-error composer-error"><WarningCircle size={17} />{error}</div>}
      <footer className="composer-footer"><span>关闭写信后仍会保留草稿；定时任务需要 iMail 服务保持运行</span></footer>
    </form>}
  </article>;
});
