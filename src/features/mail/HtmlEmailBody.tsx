import { useMemo } from 'react';

const allowedElements = new Set([
  'a', 'abbr', 'address', 'b', 'bdi', 'bdo', 'blockquote', 'br', 'caption', 'center', 'cite', 'code',
  'col', 'colgroup', 'dd', 'del', 'details', 'dfn', 'div', 'dl', 'dt', 'em', 'figcaption', 'figure',
  'font', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'i', 'img', 'ins', 'kbd', 'li', 'mark', 'ol',
  'p', 'pre', 'q', 'rp', 'rt', 'ruby', 's', 'samp', 'small', 'span', 'strike', 'strong', 'sub',
  'summary', 'sup', 'table', 'tbody', 'td', 'tfoot', 'th', 'thead', 'tr', 'tt', 'u', 'ul', 'var', 'wbr',
]);

const blockedElements = new Set([
  'applet', 'audio', 'base', 'button', 'canvas', 'embed', 'form', 'frame', 'frameset', 'head', 'iframe',
  'input', 'link', 'math', 'meta', 'noscript', 'object', 'option', 'script', 'select', 'source', 'style',
  'svg', 'template', 'textarea', 'title', 'track', 'video',
]);

const sharedAttributes = new Set(['dir', 'lang', 'style', 'title']);
const attributesByElement: Record<string, Set<string>> = {
  a: new Set(['href']),
  blockquote: new Set(['cite']),
  col: new Set(['span']),
  colgroup: new Set(['span']),
  img: new Set(['alt', 'height', 'src', 'title', 'width']),
  ol: new Set(['reversed', 'start', 'type']),
  q: new Set(['cite']),
  td: new Set(['colspan', 'rowspan']),
  th: new Set(['colspan', 'rowspan', 'scope']),
};

const unsafeCssValue = /(?:expression\s*\(|url\s*\(|image-set\s*\(|cross-fade\s*\(|element\s*\(|paint\s*\(|@import|javascript\s*:|vbscript\s*:|data\s*:|var\s*\()/i;
const unsafeCssProperty = /^(?:--|behavior$|-moz-binding$|content$|cursor$|filter$|(?:-webkit-)?mask|clip-path$|list-style-image$)/i;

export function HtmlEmailBody({ html, subject }: { html: string; subject: string }) {
  const sanitizedHtml = useMemo(() => sanitizeEmailHtml(html), [html]);

  return <div
    className="mail-html-stage"
    aria-label={`邮件正文：${subject}`}
    // The only HTML reaching this sink has passed the strict element, attribute, URL and CSS sanitizer below.
    dangerouslySetInnerHTML={{ __html: sanitizedHtml }}
  />;
}

export function sanitizeEmailHtml(html: string, Parser: typeof DOMParser = DOMParser) {
  const source = /<(?:html|body)\b/i.test(html) ? html : `<!doctype html><html><body>${html}</body></html>`;
  const document = new Parser().parseFromString(source, 'text/html');
  const elements = Array.from(document.body.querySelectorAll('*'));

  for (const element of elements) {
    const tag = element.tagName.toLocaleLowerCase();
    if (!element.isConnected) continue;
    if (blockedElements.has(tag)) {
      element.remove();
      continue;
    }
    if (!allowedElements.has(tag)) {
      element.replaceWith(...Array.from(element.childNodes));
      continue;
    }

    preserveLegacyPresentation(element, tag);
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLocaleLowerCase();
      const permitted = sharedAttributes.has(name) || attributesByElement[tag]?.has(name);
      if (!permitted) {
        element.removeAttribute(attribute.name);
        continue;
      }
      if (name === 'style') {
        const style = sanitizeInlineStyle(attribute.value, document);
        if (style) element.setAttribute('style', style);
        else element.removeAttribute('style');
      }
    }

    if (tag === 'a') sanitizeLink(element);
    if (tag === 'img') sanitizeImage(element);
    if (tag === 'blockquote' || tag === 'q') sanitizeCitation(element);
  }

  return document.body.innerHTML;
}

function preserveLegacyPresentation(element: Element, tag: string) {
  copyColorAttribute(element, 'bgcolor', 'background-color');
  copyColorAttribute(element, 'color', 'color');
  copyDimensionAttribute(element, 'width');
  copyDimensionAttribute(element, 'height');

  const align = element.getAttribute('align')?.trim().toLocaleLowerCase();
  if (align && ['left', 'right', 'center', 'justify'].includes(align)) {
    if (tag === 'table') {
      if (align === 'center') {
        setStyleFallback(element, 'margin-left', 'auto');
        setStyleFallback(element, 'margin-right', 'auto');
      } else if (align === 'left' || align === 'right') {
        setStyleFallback(element, 'float', align);
      }
    } else {
      setStyleFallback(element, 'text-align', align);
    }
  }

  const valign = element.getAttribute('valign')?.trim().toLocaleLowerCase();
  if (valign && ['baseline', 'bottom', 'middle', 'top'].includes(valign)) {
    setStyleFallback(element, 'vertical-align', valign);
  }

  if (tag === 'table') {
    const cellspacing = safeDimension(element.getAttribute('cellspacing'));
    if (cellspacing) setStyleFallback(element, 'border-spacing', cellspacing);
  }
}

function copyColorAttribute(element: Element, attribute: string, property: string) {
  const value = element.getAttribute(attribute)?.trim();
  if (value && /^(?:#[\da-f]{3,8}|(?:rgb|hsl)a?\([\d.%+\-,\s]+\)|[a-z]{1,24})$/i.test(value)) {
    setStyleFallback(element, property, value);
  }
}

function copyDimensionAttribute(element: Element, attribute: 'height' | 'width') {
  const value = safeDimension(element.getAttribute(attribute));
  if (value) setStyleFallback(element, attribute, value);
}

function safeDimension(value: string | null) {
  const normalized = value?.trim();
  if (!normalized || !/^(?:auto|0|\d+(?:\.\d+)?(?:px|%|em|rem|pt|pc|in|cm|mm|vw|vh|vmin|vmax)?)$/i.test(normalized)) return '';
  return /^\d+(?:\.\d+)?$/.test(normalized) && normalized !== '0' ? `${normalized}px` : normalized;
}

function setStyleFallback(element: Element, property: string, value: string) {
  const style = (element as HTMLElement).style;
  if (!style.getPropertyValue(property)) style.setProperty(property, value);
}

function sanitizeInlineStyle(value: string, document: Document) {
  const probe = document.createElement('span');
  probe.setAttribute('style', value);
  const safe = document.createElement('span');

  for (const property of Array.from(probe.style)) {
    const cssValue = probe.style.getPropertyValue(property).trim();
    const comparableValue = normalizeCssForInspection(cssValue);
    if (!cssValue || unsafeCssProperty.test(property) || unsafeCssValue.test(comparableValue)) continue;
    if (property.toLocaleLowerCase() === 'position' && /^(?:fixed|sticky)$/i.test(comparableValue)) continue;
    safe.style.setProperty(property, cssValue);
  }

  return safe.getAttribute('style')?.trim() ?? '';
}

function normalizeCssForInspection(value: string) {
  return value
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/\\([\da-f]{1,6})\s?/gi, (_match, code: string) => String.fromCodePoint(Number.parseInt(code, 16)))
    .replace(/\\(.)/g, '$1')
    .replace(/\s+/g, ' ')
    .trim();
}

function sanitizeLink(element: Element) {
  const href = element.getAttribute('href');
  if (!href || !isSafeUrl(href, ['http:', 'https:', 'mailto:', 'tel:'])) {
    element.removeAttribute('href');
    return;
  }
  element.setAttribute('target', '_blank');
  element.setAttribute('rel', 'noopener noreferrer');
  element.setAttribute('referrerpolicy', 'no-referrer');
}

function sanitizeImage(element: Element) {
  const src = element.getAttribute('src');
  if (!src || (!isSafeUrl(src, ['http:', 'https:']) && !isSafeRasterDataUrl(src))) {
    element.removeAttribute('src');
    return;
  }
  element.setAttribute('loading', 'lazy');
  element.setAttribute('referrerpolicy', 'no-referrer');
}

function sanitizeCitation(element: Element) {
  const cite = element.getAttribute('cite');
  if (cite && !isSafeUrl(cite, ['http:', 'https:'])) element.removeAttribute('cite');
}

function isSafeUrl(value: string, protocols: string[]) {
  try {
    return protocols.includes(new URL(value).protocol.toLocaleLowerCase());
  } catch {
    return false;
  }
}

function isSafeRasterDataUrl(value: string) {
  return /^data:image\/(?:avif|bmp|gif|jpeg|png|webp);base64,[a-z\d+/=\s]+$/i.test(value);
}
