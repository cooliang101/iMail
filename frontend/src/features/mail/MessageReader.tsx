import { lazy, Suspense, useCallback, useEffect, useRef, useState, type CSSProperties } from 'preact/compat';
import { Archive, ArrowBendDoubleUpLeft, ArrowBendUpLeft, ArrowBendUpRight, ArrowLeft, ArrowRight, ClipboardText, Clock, Code, Envelope, Eye, File, Globe, Star, Tag, Trash } from '../../components/icons';
import type { Account, Contact, Message } from '../../types';
import type { ComposeMode, MailParticipant, MessageBodyView, ParticipantRole } from '../../app-model';
import { AccountProviderMark, providerLabel } from '../../components/shared';
import { emailPlainText, MessageBody } from './MessageBody';
import { findVerificationCode } from './verification-code';
import { VerificationCodeBanner } from './VerificationCodeBanner';
import { MessageParticipants } from './MessageParticipants';
import { BilingualHtmlMessageBody } from './BilingualHtmlMessageBody';
import { BilingualMessageBody, TranslationReaderControl, type TranslationDisplayMode, type TranslationPresentation } from '../translation';
import { useI18n } from '../i18n';
import { RawMessageModal } from './RawMessageModal';
import { ConversationPanel } from './ConversationPanel';
import { MessageBodyContextMenu } from './MessageBodyContextMenu';
import { displayedMessageFilename, resolveMailBodyContextTarget, type MailBodyContextTarget } from './mail-body-context';
import { downloadRawMessage } from './raw-message-download';
import { usePlatform } from '../../platform/runtime';

const AttachmentList = lazy(() => import('../attachments/AttachmentList').then((module) => ({ default: module.AttachmentList })));

export function MessageReader({ message, account, accounts, onComposeConversationMessage, contacts, defaultBodyView, onReply, onReplyAll, onForward, onComposeSender, onFilterParticipant, onCloseMobile, onToggleFlag, onArchive, onDelete, onSnooze, onAddToWorkQueue, onManageLabels, onMarkUnread, onPrevious, onNext, onContextMenu, hasPrevious, hasNext, actionBusy }: {
  message?: Message; account?: Account; accounts: Account[]; onComposeConversationMessage: (message: Message, mode: ComposeMode) => void; contacts: Contact[]; defaultBodyView: MessageBodyView; onReply: () => void; onReplyAll: () => void; onForward: () => void; onComposeSender: (address: string) => void; onFilterParticipant: (role: ParticipantRole, participant: MailParticipant) => void; onCloseMobile: () => void; onToggleFlag: () => void; onArchive: () => void; onDelete: () => void; onSnooze: () => void; onAddToWorkQueue: () => void; onManageLabels: () => void; onMarkUnread: () => void; onPrevious: () => void; onNext: () => void; onContextMenu?: (message: Message, point: { x: number; y: number }) => void; hasPrevious: boolean; hasNext: boolean; actionBusy: boolean;
}) {
  const { t } = useI18n();
  const platform = usePlatform();
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const [bodyView, setBodyView] = useState<MessageBodyView>(defaultBodyView);
  const [translationOpen, setTranslationOpen] = useState(false);
  const [translationMounted, setTranslationMounted] = useState(false);
  const [translationMode, setTranslationMode] = useState<TranslationDisplayMode>('bilingual');
  const [translationPresentation, setTranslationPresentation] = useState<TranslationPresentation>();
  const [rawMessageOpen, setRawMessageOpen] = useState(false);
  const [bodyContextMenu, setBodyContextMenu] = useState<{ x: number; y: number; target: MailBodyContextTarget }>();
  const [bodyActionError, setBodyActionError] = useState('');
  const [bodyActionNotice, setBodyActionNotice] = useState('');
  const changeTranslationMode = useCallback((mode: TranslationDisplayMode) => {
    setTranslationMode(mode);
  }, []);
  useEffect(() => { setBodyView(defaultBodyView); setTranslationOpen(false); setTranslationMounted(false); setTranslationMode('bilingual'); setTranslationPresentation(undefined); setRawMessageOpen(false); setBodyContextMenu(undefined); setBodyActionError(''); setBodyActionNotice(''); }, [defaultBodyView, message?.id]);

  if (!message || !account) return <section className="reader empty-reader"><Envelope size={54} weight="duotone" /><h2>{t('选择一封邮件开始阅读')}</h2><p>{t('来自所有账户的邮件都会汇总在这里。')}</p></section>;
  const verificationCode = findVerificationCode([message.subject, message.preview, message.text === undefined ? '' : emailPlainText(message.text, message.html)].join('\n'));
  const performBodyAction = (action: () => Promise<void>, fallback: string) => {
    setBodyActionError('');
    setBodyActionNotice('');
    void action().catch((reason) => setBodyActionError(reason instanceof Error ? reason.message : fallback));
  };
  const copyBodyValue = (value: string) => performBodyAction(async () => {
    const content = value || bodyRef.current?.innerText || bodyRef.current?.textContent || '';
    if (!content) throw new Error('当前没有可复制的邮件内容');
    await navigator.clipboard.writeText(content);
  }, '复制失败');
  const openBodyUrl = (url: string) => performBodyAction(() => platform.openExternal(url), '链接打开失败');
  return <article className="reader">
    <div className="reader-actions" onContextMenu={(event) => { event.preventDefault(); onContextMenu?.(message, { x: event.clientX, y: event.clientY }); }}><div><button className="mobile-reader-back" title="返回邮件列表" aria-label="返回邮件列表" onClick={onCloseMobile}><ArrowLeft size={18} /></button><button data-icon-tone="neutral" title="归档" aria-label="归档邮件" disabled={actionBusy || message.mailboxRole !== 'inbox'} onClick={onArchive}><Archive size={18} /></button><button data-icon-tone="danger" title="删除" aria-label="删除邮件" disabled={actionBusy} onClick={onDelete}><Trash size={18} /></button><button data-icon-tone="warning" title="稍后处理" aria-label="稍后处理" onClick={onSnooze}><Clock size={18} /></button><button data-icon-tone="primary" title="加入处理队列" aria-label="加入邮件处理队列" onClick={onAddToWorkQueue}><ClipboardText size={18} /></button><button data-icon-tone="info" title="管理标签" aria-label="管理邮件标签" onClick={onManageLabels}><Tag size={18} /></button></div><div><button title="上一封邮件" aria-label="上一封邮件" disabled={actionBusy || !hasPrevious} onClick={onPrevious}><ArrowLeft size={18} /></button><button title="下一封邮件" aria-label="下一封邮件" disabled={actionBusy || !hasNext} onClick={onNext}><ArrowRight size={18} /></button></div></div>
    <div className="reader-scroll"><div className="reader-content">
      <div className="reader-context">
        <span className="reader-account" style={{ '--account-color': account.color } as CSSProperties}><AccountProviderMark provider={account.provider} className="reader-provider-mark" /><strong>{account.displayName}</strong><small>{account.email}</small></span>
        <span className="reader-provider-name">{providerLabel[account.provider]}</span><span>{account.group}</span>
      </div>
      <h1>{message.subject}</h1>{message.labels.length > 0 && <div className="reader-labels">{message.labels.map((label) => <span key={label}><Tag size={12} />{label}</span>)}</div>}
      <div className="sender-line"><MessageParticipants message={message} contacts={contacts} color={account.color} onCompose={onComposeSender} onFilter={onFilterParticipant} /><div className="sender-side"><time>{new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'long', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(message.date))}</time><div className="sender-actions"><button data-icon-tone="warning" title={message.flagged ? '取消星标' : '添加星标'} aria-label={message.flagged ? '取消星标' : '添加星标'} onClick={onToggleFlag}><Star size={18} weight={message.flagged ? 'fill' : 'regular'} /></button><button data-icon-tone="primary" title="回复" aria-label="回复邮件" disabled={message.text === undefined} onClick={onReply}><ArrowBendUpLeft size={18} /></button><button data-icon-tone="primary" title="回复全部" aria-label="回复全部" disabled={message.text === undefined} onClick={onReplyAll}><ArrowBendDoubleUpLeft size={18} /></button><button data-icon-tone="info" title="转发" aria-label="转发邮件" disabled={message.text === undefined} onClick={onForward}><ArrowBendUpRight size={18} /></button><button data-icon-tone="neutral" title={message.unread ? '邮件已是未读' : '标记未读'} aria-label={message.unread ? '邮件已是未读' : '标记邮件为未读'} disabled={message.unread || actionBusy} onClick={onMarkUnread}><Envelope size={18} /></button>{message.text !== undefined && <><span className="sender-action-divider" aria-hidden="true" /><div className="mail-translation-anchor"><button className={translationOpen || (translationPresentation && translationMode !== 'original') ? 'is-active mail-translation-trigger' : 'mail-translation-trigger'} data-icon-tone="primary" title={translationOpen ? '收起翻译设置' : '翻译邮件'} aria-label={translationOpen ? '收起翻译设置' : '翻译邮件'} aria-haspopup="dialog" aria-expanded={translationOpen} aria-controls="mail-translation-popover" onClick={() => { setTranslationMounted(true); setTranslationOpen((current) => !current); }}><Globe size={18} /><span>翻译</span></button>{translationMounted && <TranslationReaderControl messageId={message.id} open={translationOpen} displayMode={translationMode} onDisplayModeChange={changeTranslationMode} onPresentationChange={setTranslationPresentation} onDismiss={() => setTranslationOpen(false)} />}</div>{message.html && <button data-icon-tone="neutral" title={bodyView === 'source' ? '切换到渲染效果' : '切换到纯文本阅读'} aria-label={bodyView === 'source' ? '切换到渲染效果' : '切换到纯文本阅读'} onClick={() => setBodyView((current) => current === 'source' ? 'rendered' : 'source')}>{bodyView === 'source' ? <Eye size={18} /> : <Code size={18} />}</button>}<button data-icon-tone="neutral" title="查看原始 EML / RFC 822" aria-label="查看原始 EML / RFC 822" onClick={() => setRawMessageOpen(true)}><File size={18} /></button></>}</div></div></div>
      {verificationCode && <VerificationCodeBanner code={verificationCode} />}
      <div ref={bodyRef} className={`mail-body ${message.text === undefined ? 'mail-body-loading' : ''}`}
        onClick={(event) => {
          const target = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>('a[href]') : null;
          if (!target || !event.currentTarget.contains(target)) return;
          event.preventDefault();
          openBodyUrl(target.href);
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          event.stopPropagation();
          setBodyActionError('');
          setBodyContextMenu({ x: event.clientX, y: event.clientY, target: resolveMailBodyContextTarget(event.currentTarget, event.target, window.getSelection()) });
        }}>
        {message.text === undefined
          ? <p>{t('正在从本地缓存加载正文…')}</p>
          : translationPresentation && translationMode !== 'original'
            ? message.html && translationMode === 'bilingual'
              ? <BilingualHtmlMessageBody html={message.html} subject={message.subject} presentation={translationPresentation} />
              : <BilingualMessageBody presentation={translationPresentation} mode={translationMode} hasHtml={Boolean(message.html)} />
            : <MessageBody text={message.text} html={message.html} subject={message.subject} view={bodyView} />}
      </div>
      {bodyActionError && <div className="mail-body-action-error" role="alert">{bodyActionError}</div>}
      {bodyActionNotice && <div className="mail-body-action-notice" role="status">{bodyActionNotice}</div>}
      {message.attachments.length > 0 && <Suspense fallback={<p className="attachments-loading">{t('正在加载附件…')}</p>}><AttachmentList messageId={message.id} attachments={message.attachments} /></Suspense>}
      <ConversationPanel key={message.id} messageId={message.id} accounts={accounts} view={bodyView} onCompose={onComposeConversationMessage} />
    </div></div>
    {rawMessageOpen && <RawMessageModal message={message} onClose={() => setRawMessageOpen(false)} />}
    {bodyContextMenu && <MessageBodyContextMenu
      {...bodyContextMenu}
      platform={platform}
      bodyView={bodyView}
      hasHtml={Boolean(message.html)}
      translationActive={Boolean(translationPresentation && translationMode !== 'original')}
      onClose={() => setBodyContextMenu(undefined)}
      onCopy={copyBodyValue}
      onDownloadRaw={() => performBodyAction(() => downloadRawMessage(platform, message), '原始邮件下载失败')}
      onSaveBody={() => performBodyAction(async () => {
        const content = bodyRef.current?.innerText || bodyRef.current?.textContent || '';
        if (!content) throw new Error('当前没有可保存的邮件内容');
        await platform.saveText({ text: content, filename: displayedMessageFilename(message.subject) });
      }, '邮件正文保存失败')}
      onTranslate={() => { setTranslationMounted(true); setTranslationOpen(true); }}
      onToggleBodyView={() => setBodyView((current) => current === 'source' ? 'rendered' : 'source')}
      onOpen={openBodyUrl}
      onShare={(url) => performBodyAction(async () => {
        if (!platform.share) throw new Error('当前平台不支持系统分享');
        try {
          const outcome = await platform.share({ title: message.subject, url });
          if (outcome === 'copied') setBodyActionNotice('系统分享不可用，链接已复制到剪贴板');
        }
        catch (reason) { if (reason instanceof DOMException && reason.name === 'AbortError') return; throw reason; }
      }, '分享失败')}
      onSaveImage={(url, filename) => performBodyAction(() => platform.saveImage({ url, filename }), '图片保存失败')}
    />}
  </article>;
}
