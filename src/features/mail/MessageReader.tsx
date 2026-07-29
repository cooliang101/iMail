import { useCallback, useEffect, useRef, useState, type CSSProperties, type SyntheticEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { Archive, ArrowLeft, ArrowRight, CaretDown, Clock, Envelope, File, Star, Tag, Trash, Tray } from '@phosphor-icons/react';
import type { Account, Message } from '../../types';
import { virtualRange } from '../../virtual';
import { AccountProviderMark, initials, providerLabel, relativeTime } from '../../components/shared';

export function MessageReader({ message, account, onReply, onForward, onCloseMobile, onToggleFlag, onArchive, onDelete, onSnooze, onManageLabels, onMarkUnread, onPrevious, onNext, hasPrevious, hasNext, actionBusy }: {
  message?: Message; account?: Account; onReply: () => void; onForward: () => void; onCloseMobile: () => void; onToggleFlag: () => void; onArchive: () => void; onDelete: () => void; onSnooze: () => void; onManageLabels: () => void; onMarkUnread: () => void; onPrevious: () => void; onNext: () => void; hasPrevious: boolean; hasNext: boolean; actionBusy: boolean;
}) {
  const [moreOpen, setMoreOpen] = useState(false);
  const frameRef = useRef<HTMLIFrameElement>(null);
  const resizeObserverRef = useRef<ResizeObserver | null>(null);
  const resizeEmailFrame = useCallback(() => {
    const frame = frameRef.current;
    const document = frame?.contentDocument;
    if (!frame || !document) return;
    const body = document.body;
    const bodyStyle = body ? document.defaultView?.getComputedStyle(body) : undefined;
    const marginBottom = Number.parseFloat(bodyStyle?.marginBottom ?? '0') || 0;
    const bodyBottom = body
      ? body.getBoundingClientRect().bottom - document.documentElement.getBoundingClientRect().top + marginBottom
      : document.documentElement.scrollHeight;
    const height = body ? Array.from(body.children).reduce((bottom, child) => {
      const style = document.defaultView?.getComputedStyle(child);
      const childMarginBottom = Number.parseFloat(style?.marginBottom ?? '0') || 0;
      return Math.max(bottom, child.getBoundingClientRect().bottom - document.documentElement.getBoundingClientRect().top + childMarginBottom);
    }, bodyBottom) : bodyBottom;
    const horizontalScrollbarSpace = document.documentElement.scrollWidth > document.documentElement.clientWidth ? 18 : 2;
    frame.style.height = `${Math.max(190, height + horizontalScrollbarSpace)}px`;
  }, []);
  const handleEmailFrameLoad = useCallback((event: SyntheticEvent<HTMLIFrameElement>) => {
    resizeObserverRef.current?.disconnect();
    resizeEmailFrame();
    const document = event.currentTarget.contentDocument;
    if (!document) return;
    const observer = new ResizeObserver(resizeEmailFrame);
    if (document.body) observer.observe(document.body);
    document.querySelectorAll('img').forEach((image) => image.addEventListener('load', resizeEmailFrame, { once: true }));
    resizeObserverRef.current = observer;
  }, [resizeEmailFrame]);
  useEffect(() => () => resizeObserverRef.current?.disconnect(), []);
  if (!message || !account) return <section className="reader empty-reader"><Envelope size={54} weight="duotone" /><h2>选择一封邮件开始阅读</h2><p>来自所有账户的邮件都会汇总在这里。</p></section>;
  return <article className="reader">
    <div className="reader-actions"><div><button className="mobile-reader-back" title="返回邮件列表" aria-label="返回邮件列表" onClick={onCloseMobile}><ArrowLeft size={18} /></button><button data-icon-tone="neutral" title="归档" aria-label="归档邮件" disabled={actionBusy || message.mailboxRole !== 'inbox'} onClick={onArchive}><Archive size={18} /></button><button data-icon-tone="danger" title="删除" aria-label="删除邮件" disabled={actionBusy} onClick={onDelete}><Trash size={18} /></button><button data-icon-tone="warning" title="稍后处理" aria-label="稍后处理" onClick={onSnooze}><Clock size={18} /></button><button data-icon-tone="info" title="管理标签" aria-label="管理邮件标签" onClick={onManageLabels}><Tag size={18} /></button></div><div><button title="上一封邮件" aria-label="上一封邮件" disabled={actionBusy || !hasPrevious} onClick={onPrevious}><ArrowLeft size={18} /></button><button title="下一封邮件" aria-label="下一封邮件" disabled={actionBusy || !hasNext} onClick={onNext}><ArrowRight size={18} /></button></div></div>
    <div className="reader-scroll"><div className="reader-content">
      <div className="reader-context">
        <span className="reader-account" style={{ '--account-color': account.color } as CSSProperties}><AccountProviderMark provider={account.provider} className="reader-provider-mark" /><strong>{account.displayName}</strong><small>{account.email}</small></span>
        <span className="reader-provider-name">{providerLabel[account.provider]}</span><span>{account.group}</span>
      </div>
      <h1>{message.subject}</h1>{message.labels.length > 0 && <div className="reader-labels">{message.labels.map((label) => <span key={label}><Tag size={12} />{label}</span>)}</div>}
      <div className="sender-line"><span className="sender-avatar large" style={{ '--avatar-color': account.color } as CSSProperties}>{initials(message.from.name || message.from.address)}</span><span><strong>{message.from.name || message.from.address}</strong><small>{message.from.address} 发给 {message.to[0]?.address || account.email}</small></span><time>{new Intl.DateTimeFormat('zh-CN', { month: 'long', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(message.date))}</time><button data-icon-tone="warning" title={message.flagged ? '取消星标' : '添加星标'} aria-label={message.flagged ? '取消星标' : '添加星标'} onClick={onToggleFlag}><Star size={18} weight={message.flagged ? 'fill' : 'regular'} /></button><span className="more-wrap"><button data-icon-tone="neutral" title="更多操作" aria-label="打开更多邮件操作" aria-expanded={moreOpen} onClick={() => setMoreOpen((current) => !current)}><CaretDown size={16} /></button>{moreOpen && <span className="more-menu"><button data-icon-tone="info" disabled={message.unread} onClick={() => { setMoreOpen(false); onMarkUnread(); }}><Envelope size={15} />标记未读</button><button data-icon-tone="warning" onClick={() => { setMoreOpen(false); onSnooze(); }}><Clock size={15} />稍后处理</button><button data-icon-tone="info" onClick={() => { setMoreOpen(false); onManageLabels(); }}><Tag size={15} />管理标签</button>{message.mailboxRole === 'inbox' && <button data-icon-tone="neutral" onClick={() => { setMoreOpen(false); onArchive(); }}><Archive size={15} />归档邮件</button>}</span>}</span></div>
      <div className={`mail-body ${message.text === undefined ? 'mail-body-loading' : ''}`}>
        {message.text === undefined
          ? <p>正在从本地缓存加载正文…</p>
          : message.html
            ? <iframe ref={frameRef} className="mail-html-frame" title={`邮件正文：${message.subject}`} sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox" srcDoc={emailDocument(message.html)} onLoad={handleEmailFrameLoad} />
            : message.text.split('\n').map((line, index) => <p key={index}>{line || <br />}</p>)}
      </div>
      {message.attachments.length > 0 && <div className="attachments"><p>{message.attachments.length} 个附件 · 点击即可按需从邮箱服务器下载</p>{message.attachments.map((attachment, index) => <a key={`${attachment.filename}-${index}`} href={`/api/messages/${message.id}/attachments/${attachment.index ?? index}`} download={attachment.filename}><File size={23} weight="duotone" /><span><strong>{attachment.filename}</strong><small>{attachment.size < 1024 * 1024 ? `${Math.max(1, Math.round(attachment.size / 1024))} KB` : `${(attachment.size / 1024 / 1024).toFixed(1)} MB`}</small></span></a>)}</div>}
      <div className="reply-actions"><Button appearance="primary" icon={<ArrowLeft size={17} />} onClick={onReply}>回复</Button><Button appearance="outline" icon={<ArrowRight size={17} />} onClick={onForward}>转发</Button></div>
    </div></div>
  </article>;
}

function emailDocument(html: string) {
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="script-src 'none'; object-src 'none'; form-action 'none'"><base target="_blank"><style>html{height:auto!important;min-height:0!important;color-scheme:light}body{height:auto!important;min-height:0!important;margin:0;overflow-wrap:anywhere;color:#384944;background:#fff;font:14px/1.7 'Segoe UI Variable Text','Segoe UI',sans-serif}body>*{max-width:100%!important;box-sizing:border-box}img{max-width:100%!important;height:auto}table{max-width:100%!important}a{color:#187763}pre{white-space:pre-wrap}blockquote{margin-inline:0;padding-left:14px;border-left:3px solid #dce8e4;color:#63756f}</style></head><body>${html}</body></html>`;
}
