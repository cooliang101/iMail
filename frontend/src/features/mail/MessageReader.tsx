import { lazy, Suspense, useEffect, useState, type CSSProperties } from 'preact/compat';
import { Archive, ArrowBendUpLeft, ArrowBendUpRight, ArrowLeft, ArrowRight, Clock, Code, Envelope, Eye, Globe, Star, Tag, Trash } from '../../components/icons';
import type { Account, Contact, Message } from '../../types';
import type { MailParticipant, MessageBodyView, ParticipantRole } from '../../app-model';
import { AccountProviderMark, providerLabel } from '../../components/shared';
import { emailPlainText, MessageBody } from './MessageBody';
import { findVerificationCode } from './verification-code';
import { VerificationCodeBanner } from './VerificationCodeBanner';
import { MessageParticipants } from './MessageParticipants';
import { TranslationReaderControl } from '../translation';
import { useI18n } from '../i18n';

const AttachmentList = lazy(() => import('../attachments/AttachmentList').then((module) => ({ default: module.AttachmentList })));

export function MessageReader({ message, account, contacts, defaultBodyView, onReply, onForward, onComposeSender, onFilterParticipant, onCloseMobile, onToggleFlag, onArchive, onDelete, onSnooze, onManageLabels, onMarkUnread, onPrevious, onNext, onContextMenu, hasPrevious, hasNext, actionBusy }: {
  message?: Message; account?: Account; contacts: Contact[]; defaultBodyView: MessageBodyView; onReply: () => void; onForward: () => void; onComposeSender: (address: string) => void; onFilterParticipant: (role: ParticipantRole, participant: MailParticipant) => void; onCloseMobile: () => void; onToggleFlag: () => void; onArchive: () => void; onDelete: () => void; onSnooze: () => void; onManageLabels: () => void; onMarkUnread: () => void; onPrevious: () => void; onNext: () => void; onContextMenu?: (message: Message, point: { x: number; y: number }) => void; hasPrevious: boolean; hasNext: boolean; actionBusy: boolean;
}) {
  const { t } = useI18n();
  const [bodyView, setBodyView] = useState<MessageBodyView>(defaultBodyView);
  const [translationOpen, setTranslationOpen] = useState(false);
  useEffect(() => { setBodyView(defaultBodyView); setTranslationOpen(false); }, [defaultBodyView, message?.id]);

  if (!message || !account) return <section className="reader empty-reader"><Envelope size={54} weight="duotone" /><h2>{t('选择一封邮件开始阅读')}</h2><p>{t('来自所有账户的邮件都会汇总在这里。')}</p></section>;
  const verificationCode = findVerificationCode([message.subject, message.preview, message.text === undefined ? '' : emailPlainText(message.text, message.html)].join('\n'));
  return <article className="reader">
    <div className="reader-actions" onContextMenu={(event) => { event.preventDefault(); onContextMenu?.(message, { x: event.clientX, y: event.clientY }); }}><div><button className="mobile-reader-back" title="返回邮件列表" aria-label="返回邮件列表" onClick={onCloseMobile}><ArrowLeft size={18} /></button><button data-icon-tone="neutral" title="归档" aria-label="归档邮件" disabled={actionBusy || message.mailboxRole !== 'inbox'} onClick={onArchive}><Archive size={18} /></button><button data-icon-tone="danger" title="删除" aria-label="删除邮件" disabled={actionBusy} onClick={onDelete}><Trash size={18} /></button><button data-icon-tone="warning" title="稍后处理" aria-label="稍后处理" onClick={onSnooze}><Clock size={18} /></button><button data-icon-tone="info" title="管理标签" aria-label="管理邮件标签" onClick={onManageLabels}><Tag size={18} /></button></div><div><button title="上一封邮件" aria-label="上一封邮件" disabled={actionBusy || !hasPrevious} onClick={onPrevious}><ArrowLeft size={18} /></button><button title="下一封邮件" aria-label="下一封邮件" disabled={actionBusy || !hasNext} onClick={onNext}><ArrowRight size={18} /></button></div></div>
    <div className="reader-scroll"><div className="reader-content">
      <div className="reader-context">
        <span className="reader-account" style={{ '--account-color': account.color } as CSSProperties}><AccountProviderMark provider={account.provider} className="reader-provider-mark" /><strong>{account.displayName}</strong><small>{account.email}</small></span>
        <span className="reader-provider-name">{providerLabel[account.provider]}</span><span>{account.group}</span>
      </div>
      <h1>{message.subject}</h1>{message.labels.length > 0 && <div className="reader-labels">{message.labels.map((label) => <span key={label}><Tag size={12} />{label}</span>)}</div>}
      <div className="sender-line"><MessageParticipants message={message} contacts={contacts} color={account.color} onCompose={onComposeSender} onFilter={onFilterParticipant} /><div className="sender-side"><time>{new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'long', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(message.date))}</time><div className="sender-actions"><button data-icon-tone="warning" title={message.flagged ? '取消星标' : '添加星标'} aria-label={message.flagged ? '取消星标' : '添加星标'} onClick={onToggleFlag}><Star size={18} weight={message.flagged ? 'fill' : 'regular'} /></button><button data-icon-tone="primary" title="回复" aria-label="回复邮件" onClick={onReply}><ArrowBendUpLeft size={18} /></button><button data-icon-tone="info" title="转发" aria-label="转发邮件" onClick={onForward}><ArrowBendUpRight size={18} /></button><button data-icon-tone="neutral" title={message.unread ? '邮件已是未读' : '标记未读'} aria-label={message.unread ? '邮件已是未读' : '标记邮件为未读'} disabled={message.unread || actionBusy} onClick={onMarkUnread}><Envelope size={18} /></button>{message.text !== undefined && <><span className="sender-action-divider" aria-hidden="true" /><button className={translationOpen ? 'is-active' : ''} data-icon-tone="primary" title={translationOpen ? '关闭邮件翻译' : '翻译邮件'} aria-label={translationOpen ? '关闭邮件翻译' : '翻译邮件'} aria-pressed={translationOpen} onClick={() => setTranslationOpen((current) => !current)}><Globe size={18} /></button>{message.html && <button data-icon-tone="neutral" title={bodyView === 'source' ? '切换到渲染效果' : '切换到原始内容'} aria-label={bodyView === 'source' ? '切换到渲染效果' : '切换到原始内容'} onClick={() => setBodyView((current) => current === 'source' ? 'rendered' : 'source')}>{bodyView === 'source' ? <Eye size={18} /> : <Code size={18} />}</button>}</>}</div></div></div>
      {verificationCode && <VerificationCodeBanner code={verificationCode} />}
      {translationOpen && <TranslationReaderControl messageId={message.id} onClose={() => setTranslationOpen(false)} />}
      <div className={`mail-body ${message.text === undefined ? 'mail-body-loading' : ''}`}>
        {message.text === undefined
          ? <p>{t('正在从本地缓存加载正文…')}</p>
          : <MessageBody text={message.text} html={message.html} subject={message.subject} view={bodyView} />}
      </div>
      {message.attachments.length > 0 && <Suspense fallback={<p className="attachments-loading">{t('正在加载附件…')}</p>}><AttachmentList messageId={message.id} attachments={message.attachments} /></Suspense>}
    </div></div>
  </article>;
}
