import { useMemo } from 'preact/compat';
import { sanitizeEmailHtml } from './sanitize-email-html';

export { sanitizeEmailHtml } from './sanitize-email-html';

export function HtmlEmailBody({ html, subject }: { html: string; subject: string }) {
  const sanitizedHtml = useMemo(() => sanitizeEmailHtml(html), [html]);
  return <div className="mail-html-stage" aria-label={`邮件正文：${subject}`}
    // The only HTML reaching this sink has passed the strict element, attribute, URL and CSS sanitizer.
    dangerouslySetInnerHTML={{ __html: sanitizedHtml }} />;
}
