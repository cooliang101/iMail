import { DOMParser } from 'linkedom';
import { describe, expect, it } from 'vitest';
import { sanitizeEmailHtml } from './HtmlEmailBody';

const sanitize = (html: string) => sanitizeEmailHtml(html, DOMParser as unknown as typeof globalThis.DOMParser);

describe('rendered email sanitization', () => {
  it('extracts body children and removes executable or embedded content', () => {
    const result = sanitize('<html><head><style>.red{color:red}</style></head><body><div>safe<script>alert(1)</script></div><iframe src="https://evil.test"></iframe><form><input value="x"></form></body></html>');
    expect(result).toBe('<div>safe</div>');
  });

  it('removes classes, ids, event handlers and unsafe URLs while retaining safe inline styles', () => {
    const result = sanitize('<div id="app" class="card" onclick="alert(1)" style="color:red; position:absolute; inset:0; z-index:9999; zoom:4; transform:scale(5); background-image:u\\72l(https://evil.test/pixel)"><a href="javascript:alert(1)">bad</a><a href="https://example.com/x">safe</a></div>');
    expect(result).toContain('<div style="color:red">');
    expect(result).not.toMatch(/class=|id=|onclick=|position|inset|z-index|zoom|transform|background-image|javascript/i);
    expect(result).toContain('href="https://example.com/x"');
    expect(result).toContain('target="_blank"');
    expect(result).toContain('rel="noopener noreferrer"');
  });

  it('allows safe raster images but rejects SVG data and removes dangerous image attributes', () => {
    const result = sanitize('<img class="hero" onerror="alert(1)" src="data:image/png;base64,AAAA"><img src="data:image/svg+xml;base64,PHN2Zz4="><img src="file:///secret">');
    expect(result).toContain('src="data:image/png;base64,AAAA"');
    expect(result).not.toMatch(/class=|onerror=|svg\+xml|file:/i);
  });

  it('keeps image-only action links usable when the remote image cannot be displayed', () => {
    const result = sanitize('<a href="https://signup.example.test/verify/token"><img src="https://signup.example.test/images/verify-email-button.png"></a>');
    expect(result).toContain('href="https://signup.example.test/verify/token"');
    expect(result).toContain('target="_blank"');
    expect(result).toContain('alt="Verify email"');
  });

  it('unwraps unknown formatting elements without preserving their attributes', () => {
    expect(sanitize('<custom-box class="x"><strong style="font-weight:700">kept</strong></custom-box>'))
      .toBe('<strong style="font-weight:700">kept</strong>');
  });

  it('converts legacy email presentation attributes to sanitized inline styles', () => {
    const result = sanitize('<table class="bg-blue" bgcolor="#0a007d" width="100%" cellspacing="0"><tr><td align="center" valign="top">Blue panel</td></tr></table>');
    expect(result).toContain('style="background-color:#0a007d;width:100%;border-spacing:0"');
    expect(result).toContain('style="text-align:center;vertical-align:top"');
    expect(result).not.toMatch(/class=|bgcolor=|\swidth=|cellspacing=|\salign=|valign=/i);
  });
});
