import { useState } from 'react';
import { Code, Eye } from '@phosphor-icons/react';
import { HtmlEmailBody } from './HtmlEmailBody';
import type { MessageBodyView } from '../../app-model';

export function MessageBody({ text, html, subject, defaultView }: { text: string; html?: string; subject: string; defaultView: MessageBodyView }) {
  const [view, setView] = useState<MessageBodyView>(defaultView);
  const source = html || text;

  return <>
    <div className="mail-body-view-switch" role="group" aria-label="邮件内容查看方式">
      <button type="button" className={view === 'source' ? 'is-active' : ''} aria-pressed={view === 'source'} onClick={() => setView('source')}><Code size={14} />原始内容</button>
      <button type="button" className={view === 'rendered' ? 'is-active' : ''} aria-pressed={view === 'rendered'} onClick={() => setView('rendered')}><Eye size={14} />渲染效果</button>
    </div>
    {view === 'source'
      ? <pre className="mail-source-body">{source}</pre>
      : html
        ? <HtmlEmailBody html={html} subject={subject} />
        : <div className="mail-text-body">{text.split('\n').map((line, index) => <p key={index}>{line || <br />}</p>)}</div>}
  </>;
}
