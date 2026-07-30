import { describe, expect, it } from 'vitest';
import { senderLogoInternals } from './sender-logo.js';
import type { CachedMessage } from './types.js';

const message = (html: string): CachedMessage => ({
  id: 'm1', accountId: 'a1', mailbox: 'INBOX', uid: 1, from: { name: 'QQ 音乐', address: 'notice@qq.com' }, to: [],
  subject: '到期提醒', preview: '', text: '', html, date: new Date().toISOString(), unread: true, flagged: false, hasAttachments: false, attachments: [],
});

describe('sender logo discovery', () => {
  it('prefers safe website origins from mail content and filters tracking links', () => {
    const candidates = senderLogoInternals.siteCandidates(message('<a href="https://track.example.net/click/1">x</a><a href="https://y.qq.com/n/ryqq">官网</a>'));
    expect(candidates.map((item) => item.href)).toEqual(['https://y.qq.com/', 'https://qq.com/', 'https://www.qq.com/']);
  });

  it('uses exact-domain cache keys with a shared registrable-domain fallback', () => {
    const nested = message('');
    nested.from.address = 'no-reply@accounts.google.com';
    const root = message('');
    root.from.address = 'noreply-accounts@google.com';
    expect(senderLogoInternals.senderKey(nested)).toBe('domain:accounts.google.com');
    expect(senderLogoInternals.senderKey(root)).toBe('domain:google.com');
    expect(senderLogoInternals.rootSenderKey(nested)).toBe('domain:google.com');
    expect(senderLogoInternals.rootSenderKey(root)).toBe('domain:google.com');
    expect(senderLogoInternals.contactDomain('hello@service.example.co.uk')?.registrable).toBe('example.co.uk');
  });

  it('prioritizes links belonging to the sender registrable domain', () => {
    const source = message('<a href="https://news.other.test/article">外部</a><a href="https://accounts.google.com/login">账户</a>');
    source.from.address = 'no-reply@google.com';
    expect(senderLogoInternals.siteCandidates(source).map((item) => item.href)).toEqual(['https://accounts.google.com/', 'https://google.com/', 'https://www.google.com/']);
  });

  it('ignores HTML namespace URLs and rejects Cloudflare challenge pages', () => {
    const source = message('<html xmlns="http://www.w3.org/1999/xhtml"><body>mail</body></html>');
    source.from.address = 'help@epicgames.com';
    expect(senderLogoInternals.siteCandidates(source).map((item) => item.href)).toEqual(['https://epicgames.com/', 'https://www.epicgames.com/']);
    expect(senderLogoInternals.isChallengePage('<title>Just a moment...</title><script src="/cdn-cgi/challenge-platform/x"></script>')).toBe(true);
  });

  it('discovers declared icons and a favicon fallback', () => {
    const icons = senderLogoInternals.discoverIcons('<link rel="apple-touch-icon" href="/logo.png"><link rel="stylesheet" href="/x.css">', new URL('https://example.com/news'));
    expect(icons.map((item) => item.href)).toEqual(['https://example.com/logo.png', 'https://example.com/favicon.ico']);
  });

  it('expires failed attempts and negative cache entries after one day', () => {
    const now = Date.parse('2026-07-30T00:00:00.000Z');
    expect(senderLogoInternals.FAILURE_CACHE_TTL_MS).toBe(24 * 60 * 60_000);
    expect(senderLogoInternals.isFreshFailure('2026-07-29T01:00:00.000Z', now)).toBe(true);
    expect(senderLogoInternals.isFreshFailure('2026-07-28T23:59:59.999Z', now)).toBe(false);
    expect(senderLogoInternals.isFreshFailure('invalid', now)).toBe(false);
  });

  it('recognizes only Cloudflare 403 responses as permanent failures', () => {
    expect(senderLogoInternals.isCloudflareForbidden(new Response('', { status: 403, headers: { server: 'cloudflare', 'cf-ray': 'test' } }))).toBe(true);
    expect(senderLogoInternals.isCloudflareForbidden(new Response('', { status: 403, headers: { server: 'nginx' } }))).toBe(false);
    expect(senderLogoInternals.isCloudflareForbidden(new Response('', { status: 429, headers: { server: 'cloudflare' } }))).toBe(false);
  });

  it('rejects private, loopback and link-local addresses', () => {
    expect(senderLogoInternals.isPublicIp('127.0.0.1')).toBe(false);
    expect(senderLogoInternals.isPublicIp('192.168.1.1')).toBe(false);
    expect(senderLogoInternals.isPublicIp('::1')).toBe(false);
    expect(senderLogoInternals.isPublicIp('8.8.8.8')).toBe(true);
  });

  it('pins the validated IP while preserving the original HTTP host and TLS server name', () => {
    const options = senderLogoInternals.pinnedRequestOptions(
      new URL('https://logos.example.com/path?q=1'), { address: '203.0.113.8', family: 4 }, 'image/png', AbortSignal.timeout(1_000),
    );
    expect(options).toMatchObject({
      hostname: '203.0.113.8', family: 4, port: 443, path: '/path?q=1', servername: 'logos.example.com',
      headers: { Host: 'logos.example.com', Accept: 'image/png' },
    });
  });
});
