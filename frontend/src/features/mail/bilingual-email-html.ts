import type { TranslationPresentation } from '../translation';
import { sanitizeEmailHtml } from './sanitize-email-html';

const translationBlockSelector = 'address,dd,dt,figcaption,h1,h2,h3,h4,h5,h6,li,p,pre,td,th,div';
const listPrefix = /^\s*(?:[-*•]|\d+[.)])\s*/;

export type BilingualEmailHtml = {
  html: string;
  mappedSegmentCount: number;
};

export function buildBilingualEmailHtml(
  html: string,
  presentation: TranslationPresentation,
  Parser: typeof DOMParser = DOMParser,
): BilingualEmailHtml | undefined {
  const sanitized = sanitizeEmailHtml(html, Parser);
  const document = new Parser().parseFromString(`<!doctype html><html><body>${sanitized}</body></html>`, 'text/html');
  const candidates = collectTranslationBlocks(document);
  const translatedById = new Map(presentation.artifact?.segments.map((segment) => [segment.id, segment.text]));
  let cursor = 0;

  for (const segment of presentation.document.segments) {
    const translated = translatedById.get(segment.id);
    const listMatch = segment.kind === 'list-item' ? findListMatch(candidates, cursor, segment.text) : undefined;
    const match = listMatch ?? findBlockMatch(candidates, cursor, segment.text);
    if (!match) return undefined;

    if (listMatch && translated) {
      const translatedLines = meaningfulLines(translated).map((line) => line.replace(listPrefix, ''));
      if (translatedLines.length === listMatch.elements.length) {
        listMatch.elements.forEach((element, index) => appendTranslation(document, element, translatedLines[index], presentation.targetLanguage));
      } else {
        appendTranslation(document, listMatch.elements[listMatch.elements.length - 1], translated, presentation.targetLanguage);
      }
    } else if (translated && match.elements.length > 1) {
      const sourceLines = meaningfulLines(segment.text);
      const translatedLines = meaningfulLines(translated);
      if (sourceLines.length === match.elements.length && translatedLines.length === match.elements.length) {
        match.elements.forEach((element, index) => appendTranslation(document, element, translatedLines[index], presentation.targetLanguage));
      } else {
        appendTranslation(document, match.elements[match.elements.length - 1], translated, presentation.targetLanguage);
      }
    } else {
      appendTranslation(document, match.elements[match.elements.length - 1], translated, presentation.targetLanguage);
    }
    cursor = match.end;
  }

  if (cursor !== candidates.length) return undefined;

  return { html: document.body.innerHTML, mappedSegmentCount: presentation.document.segments.length };
}

function collectTranslationBlocks(document: Document) {
  return Array.from(document.body.querySelectorAll(translationBlockSelector)).filter((element) => {
    if (element.closest('blockquote') || isVisuallyHidden(element)) return false;
    return !element.querySelector(translationBlockSelector);
  }).filter((element) => comparableText(visibleText(element)));
}

function findListMatch(candidates: Element[], cursor: number, source: string) {
  const lines = meaningfulLines(source).map((line) => comparableText(line.replace(listPrefix, '')));
  if (!lines.length) return undefined;
  const elements = candidates.slice(cursor, cursor + lines.length);
  if (elements.length === lines.length && elements.every((element, index) => Boolean(element.closest('li')) && comparableText(visibleText(element)) === lines[index])) {
    return { elements, end: cursor + elements.length };
  }
  return undefined;
}

function findBlockMatch(candidates: Element[], cursor: number, source: string) {
  const comparableSource = comparableText(source);
  let combined = '';
  for (let end = cursor; end < candidates.length; end += 1) {
    combined = `${combined} ${visibleText(candidates[end])}`;
    const comparableCombined = comparableText(combined);
    if (comparableCombined === comparableSource) return { elements: candidates.slice(cursor, end + 1), end: end + 1 };
    if (comparableCombined.length > comparableSource.length * 1.25 + 16) break;
  }
  return undefined;
}

function isVisuallyHidden(element: Element) {
  for (let current: Element | null = element; current && current.tagName.toLocaleLowerCase() !== 'body'; current = current.parentElement) {
    if (current.hasAttribute('hidden') || current.getAttribute('aria-hidden') === 'true') return true;
    const style = (current as HTMLElement).style;
    if (style.display === 'none' || ['hidden', 'collapse'].includes(style.visibility) || Number.parseFloat(style.opacity) === 0 || style.getPropertyValue('content-visibility') === 'hidden') return true;
    const clipped = ['hidden', 'clip'].includes(style.overflow) || ['hidden', 'clip'].includes(style.overflowY);
    if (clipped && (isZeroDimension(style.height) || isZeroDimension(style.maxHeight))) return true;
    if (isZeroDimension(style.fontSize) && isZeroDimension(style.lineHeight)) return true;
  }
  return false;
}

function isZeroDimension(value: string) {
  return /^0(?:\.0+)?(?:px|pt|pc|in|cm|mm|em|rem|%)?$/i.test(value.trim());
}

function appendTranslation(document: Document, source: Element, translated: string | undefined, targetLanguage: string) {
  const output = document.createElement('div');
  output.className = translated ? 'mail-inline-translation' : 'mail-inline-translation is-pending';
  output.setAttribute('lang', targetLanguage);
  output.setAttribute('data-imail-translation', 'true');
  if (translated) {
    output.textContent = translated;
  } else {
    output.setAttribute('role', 'status');
    output.setAttribute('aria-label', '正在翻译此段');
    output.append(document.createElement('span'), document.createElement('span'));
  }
  if (['li', 'td', 'th'].includes(source.tagName.toLocaleLowerCase())) source.append(output);
  else source.insertAdjacentElement('afterend', output);
}

function visibleText(element: Element) {
  const parts: string[] = [];
  for (const node of Array.from(element.childNodes)) {
    if (node.nodeType === 3) parts.push(node.textContent ?? '');
    else if (node.nodeType === 1 && (node as Element).tagName.toLocaleLowerCase() === 'br') parts.push('\n');
    else if (node.nodeType === 1) parts.push(visibleText(node as Element));
  }
  return parts.join('');
}

function meaningfulLines(value: string) {
  return value.replace(/\r\n?/g, '\n').split('\n').map((line) => line.trim()).filter(Boolean);
}

function comparableText(value: string) {
  return value.replace(/\u00a0/g, ' ').replace(/\s+/g, ' ').trim();
}
