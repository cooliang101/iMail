import { HtmlEmailBody } from './HtmlEmailBody';
import type { MessageBodyView } from '../../app-model';

export function MessageBody({ text, html, subject, view }: { text: string; html?: string; subject: string; view: MessageBodyView }) {
  const plainText = emailPlainText(text, html);
  return view === 'rendered' && html
    ? <HtmlEmailBody html={html} subject={subject} />
    : <div className="mail-plain-body">{plainText
      ? plainText.split(/\n{2,}/).map((block, index) => <p className={isPreformattedBlock(block) ? 'mail-plain-preformatted' : undefined} key={`${index}-${block.slice(0, 24)}`}>{block}</p>)
      : <p>（邮件没有可显示的文本内容）</p>}</div>;
}

function isPreformattedBlock(value: string) {
  const lines = value.split('\n');
  return lines.length > 1 && lines.some((line) => /^\s*(?:>|[-*+]\s|\d+[.)]\s|--\s*$)/.test(line));
}

export function emailPlainText(text: string, html?: string) {
  const source = text.trim() ? text : (html ?? '');
  if (!source.includes('<') && !source.includes('&')) return normalizePlainText(source);
  if (typeof DOMParser !== 'undefined') {
    const document = new DOMParser().parseFromString(source, 'text/html');
    document.querySelectorAll('head, style, script, template, noscript, svg, canvas, object').forEach((node) => node.remove());
    document.querySelectorAll('br').forEach((node) => node.replaceWith('\n'));
    document.querySelectorAll('p, div, section, article, header, footer, li, tr, blockquote, h1, h2, h3, h4, h5, h6').forEach((node) => node.append('\n'));
    return normalizePlainText(document.body.textContent ?? '');
  }
  return normalizePlainText(decodeHtmlEntities(source
    .replace(/<(head|style|script|template|noscript|svg|canvas|object)\b[^>]*>[\s\S]*?<\/\1\s*>/gi, '')
    .replace(/<br\s*\/?>/gi, '\n')
    .replace(/<\/(p|div|section|article|header|footer|li|tr|blockquote|h[1-6])\s*>/gi, '\n')
    .replace(/<[^>]+>/g, '')));
}

function decodeHtmlEntities(value: string) {
  const named: Record<string, string> = { amp: '&', apos: "'", gt: '>', lt: '<', nbsp: ' ', quot: '"' };
  return value.replace(/&(#x[\da-f]+|#\d+|[a-z]+);/gi, (entity, code: string) => {
    if (code[0] !== '#') return named[code.toLocaleLowerCase()] ?? entity;
    const point = code[1]?.toLocaleLowerCase() === 'x' ? Number.parseInt(code.slice(2), 16) : Number.parseInt(code.slice(1), 10);
    return Number.isFinite(point) && point >= 0 && point <= 0x10ffff ? String.fromCodePoint(point) : entity;
  });
}

function normalizePlainText(value: string) {
  return value.replace(/\r\n?/g, '\n').replace(/[\t ]+\n/g, '\n').replace(/\n[\t ]+/g, '\n').replace(/\n{3,}/g, '\n\n').trim();
}
