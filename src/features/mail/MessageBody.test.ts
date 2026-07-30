import { describe, expect, it } from 'vitest';
import { emailPlainText } from './MessageBody';

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
});
