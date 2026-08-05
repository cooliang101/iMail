import { describe, expect, it } from 'vitest';
import { proxyInputFromForm } from './ProxyFields';

describe('proxy form input', () => {
  it('uses another account as a reusable proxy source', () => {
    const form = new FormData();
    form.set('proxyEnabled', 'on');
    form.set('proxySourceAccountId', '11111111-1111-4111-8111-111111111111');
    expect(proxyInputFromForm(form)).toEqual({ enabled: true, sourceAccountId: '11111111-1111-4111-8111-111111111111' });
  });

  it('keeps manual proxy fields when no source is selected', () => {
    const form = new FormData();
    form.set('proxyEnabled', 'on');
    form.set('proxyProtocol', 'socks5');
    form.set('proxyHost', 'proxy.example.com');
    form.set('proxyPort', '1080');
    form.set('proxyUsername', 'proxy-user');
    expect(proxyInputFromForm(form)).toEqual({ enabled: true, protocol: 'socks5', host: 'proxy.example.com', port: 1080, username: 'proxy-user', password: undefined });
  });

  it('disables proxy when the checkbox is clear', () => {
    expect(proxyInputFromForm(new FormData())).toEqual({ enabled: false });
  });
});
