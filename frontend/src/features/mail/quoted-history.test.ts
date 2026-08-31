import { DOMParser } from 'linkedom';
import { describe, expect, it } from 'vitest';
import { foldHtmlHistory, splitPlainHistory } from './quoted-history';
import { sanitizeEmailHtml } from './sanitize-email-html';

const parser = DOMParser as unknown as typeof globalThis.DOMParser;
describe('quoted history', () => {
  it('folds outer reply blocks once without discarding inline responses', () => {
    const result = foldHtmlHistory('<p>new</p><blockquote>old<blockquote>older</blockquote></blockquote><p>inline response</p>', parser);
    expect(result.match(/<details>/g)).toHaveLength(1);
    expect(result).toContain('<p>inline response</p>');
    expect(result).toContain('older');
  });
  it('recognizes provider wrappers and still sanitizes hostile content', () => {
    const result = sanitizeEmailHtml(foldHtmlHistory('<div class="gmail_quote" onclick="bad()">old<img src="file:///secret"><script>bad()</script></div>', parser), parser);
    expect(result).toContain('<details>');
    expect(result).toContain('old');
    expect(result).not.toMatch(/onclick|file:|script|class=/);
  });
  it('does not hide unquoted text between quote runs', () => {
    expect(splitPlainHistory('answer\n> first question\ninline reply\n> second question')).toEqual([
      { quoted: false, text: 'answer' }, { quoted: true, text: '> first question' },
      { quoted: false, text: 'inline reply' }, { quoted: true, text: '> second question' },
    ]);
    expect(splitPlainHistory('answer\n----- 原邮件 -----\nold content')[1]).toEqual({ quoted: true, text: '----- 原邮件 -----\nold content' });
  });
});
