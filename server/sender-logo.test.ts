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
    expect(candidates.map((item) => item.href)).toEqual(['https://y.qq.com/', 'https://qq.com/']);
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
    expect(senderLogoInternals.siteCandidates(source).map((item) => item.href)).toEqual(['https://accounts.google.com/', 'https://google.com/']);
  });

  it('ignores HTML namespace URLs and rejects Cloudflare challenge pages', () => {
    const source = message('<html xmlns="http://www.w3.org/1999/xhtml"><body>mail</body></html>');
    source.from.address = 'help@epicgames.com';
    expect(senderLogoInternals.siteCandidates(source).map((item) => item.href)).toEqual(['https://epicgames.com/']);
    expect(senderLogoInternals.isChallengePage('<title>Just a moment...</title><script src="/cdn-cgi/challenge-platform/x"></script>')).toBe(true);
  });

  it('discovers declared icons and a favicon fallback', () => {
    const icons = senderLogoInternals.discoverIcons('<link rel="apple-touch-icon" href="/logo.png"><link rel="stylesheet" href="/x.css">', new URL('https://example.com/news'));
    expect(icons.map((item) => item.href)).toEqual(['https://example.com/logo.png', 'https://example.com/favicon.ico']);
  });

  it('rejects private, loopback and link-local addresses', () => {
    expect(senderLogoInternals.isPublicIp('127.0.0.1')).toBe(false);
    expect(senderLogoInternals.isPublicIp('192.168.1.1')).toBe(false);
    expect(senderLogoInternals.isPublicIp('::1')).toBe(false);
    expect(senderLogoInternals.isPublicIp('8.8.8.8')).toBe(true);
  });
});
