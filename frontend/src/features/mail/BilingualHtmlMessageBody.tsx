import { useMemo } from 'preact/compat';
import { BilingualMessageBody, type TranslationPresentation } from '../translation';
import { buildBilingualEmailHtml } from './bilingual-email-html';

export function BilingualHtmlMessageBody({ html, subject, presentation }: {
  html: string;
  subject: string;
  presentation: TranslationPresentation;
}) {
  const result = useMemo(() => buildBilingualEmailHtml(html, presentation), [html, presentation]);
  if (!result) return <BilingualMessageBody presentation={presentation} mode="bilingual" hasHtml />;

  return <div className="mail-bilingual-html-body" aria-busy={presentation.busy}>
    <div className="mail-bilingual-layout-note"><span>译文已按原始邮件排版显示在对应段落下方。</span></div>
    <div className="mail-html-stage" aria-label={`双语邮件正文：${subject}`}
      // The original HTML is strictly sanitized before plain-text translation nodes are inserted.
      dangerouslySetInnerHTML={{ __html: result.html }} />
    {presentation.document.omittedQuotedText && <p className="mail-bilingual-omitted">引用历史保留原文，不会发送给翻译服务。</p>}
  </div>;
}
