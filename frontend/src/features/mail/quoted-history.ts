/** Recognition is presentation-only; the result MUST still pass the HTML sanitizer. */
export function foldHtmlHistory(html: string, Parser: typeof DOMParser = DOMParser) {
  const source = /<(?:html|body)\b/i.test(html) ? html : `<!doctype html><html><body>${html}</body></html>`;
  const document = new Parser().parseFromString(source, 'text/html');
  const selector = 'blockquote, .gmail_quote, .yahoo_quoted, [type="cite"]';
  const quotes = Array.from(document.body.querySelectorAll(selector)).filter(node => !node.parentElement?.closest(selector) && !node.closest('details'));
  for (const quote of quotes) {
    const details = document.createElement('details');
    const summary = document.createElement('summary');
    summary.textContent = '展开引用内容';
    quote.replaceWith(details);
    details.append(summary, quote);
  }
  return document.body.innerHTML;
}

export function splitPlainHistory(text: string) {
  const sections: Array<{ quoted: boolean; text: string }> = [];
  let history = false;
  for (const line of text.split('\n')) {
    if (/^-{3,}\s*(?:原邮件|转发邮件|Original Message|Forwarded message)\s*-{3,}$/i.test(line.trim())) history = true;
    const quoted = history || /^\s*>/.test(line);
    const previous = sections.at(-1);
    if (previous && (previous.quoted === quoted || !line.trim())) previous.text += '\n' + line;
    else sections.push({ quoted, text: line });
  }
  return sections.filter(section => section.text.trim());
}
