import { DOMParser, parseHTML } from 'linkedom';
import { describe, expect, it } from 'vitest';
import { emailPlainText } from './MessageBody';
import { sanitizePlainEmailHtml } from './sanitize-email-html';

const sanitizePlain = (html: string) => sanitizePlainEmailHtml(html, DOMParser as unknown as typeof globalThis.DOMParser);

describe('email plain-text presentation', () => {
  it('removes HTML elements, styles and scripts while preserving readable text', () => {
    expect(emailPlainText('<style>.hidden { display:none }</style><div>123</div><p>Hello <strong>world</strong><br>next</p><script>alert(1)</script>'))
      .toBe('123\nHello world\nnext');
  });

  it('decodes entities and uses HTML when the text alternative is empty', () => {
    expect(emailPlainText('', '<div>A &amp; B&nbsp;&#x4E2D;&#25991;</div>')).toBe('A & B 中文');
  });

  it('keeps existing plain-text line breaks', () => {
    expect(emailPlainText('first\r\nsecond\n\n\nthird')).toBe('first\nsecond\n\nthird');
  });

  it('keeps safe HTML-only sign-in links without styles or remote resources', () => {
    const sanitized = sanitizePlain('<span style="display:none">hidden preheader</span><table style="width:640px"><tr><td bgcolor="#141413"><p><a clicktracking="off" href="https://claude.ai/magic-link#token:payload==" style="display:inline-block;background:#141413;color:white">Sign in</a><img src="https://tracker.example/open.gif"></p></td></tr></table>');
    const { document } = parseHTML(`<div class="mail-plain-body">${sanitized}</div>`);
    const link = document.querySelector('.mail-plain-body a');
    expect(link?.textContent).toBe('Sign in');
    expect(link?.getAttribute('href')).toBe('https://claude.ai/magic-link#token:payload==');
    expect(link?.getAttribute('target')).toBe('_blank');
    expect(link?.getAttribute('rel')).toBe('noopener noreferrer');
    expect(link?.getAttribute('referrerpolicy')).toBe('no-referrer');
    expect(document.querySelector('.mail-plain-body img')).toBeNull();
    expect(document.querySelector('.mail-plain-body [style], .mail-plain-body [bgcolor], .mail-plain-body [clicktracking]')).toBeNull();
    expect(document.querySelector('.mail-plain-body')?.textContent).not.toContain('hidden preheader');
  });

  it('does not make unsafe links clickable in the plain view', () => {
    const { document } = parseHTML(`<div class="mail-plain-body">${sanitizePlain('<p><a href="javascript:alert(1)">Open</a></p>')}</div>`);
    expect(document.querySelector('.mail-plain-body a')?.hasAttribute('href')).toBe(false);
  });
});
