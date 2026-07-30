import { useEffect, useId, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { EnvelopeSimple, MicrosoftOutlookLogo } from '@phosphor-icons/react';
import { siGmail, siIcloud, siQq } from 'simple-icons';
import type { ProviderId } from '../types';
import type { ContactLogo } from '../types';

export const providerLabel: Record<ProviderId, string> = { outlook: 'Outlook', gmail: 'Gmail', qq: 'QQ', yahoo: 'Yahoo', hotmail: 'Hotmail', icloud: 'iCloud', custom: 'IMAP' };
export const providers: Array<{ id: ProviderId; name: string; oauthKey?: 'google' | 'microsoft' | 'yahoo'; helpUrl?: string }> = [
  { id: 'outlook', name: 'Outlook', oauthKey: 'microsoft' }, { id: 'gmail', name: 'Gmail', oauthKey: 'google' },
  { id: 'qq', name: 'QQ 邮箱' }, { id: 'yahoo', name: 'Yahoo', oauthKey: 'yahoo' },
  { id: 'hotmail', name: 'Hotmail', oauthKey: 'microsoft' }, { id: 'icloud', name: 'iCloud' },
  { id: 'custom', name: '更多邮箱' },
];

const simpleProviderIcons: Partial<Record<ProviderId, { path: string; hex: string; title: string }>> = {
  gmail: siGmail, qq: siQq, icloud: siIcloud,
};

export function ProviderIcon({ provider }: { provider: ProviderId }) {
  const icon = simpleProviderIcons[provider];
  if (icon) return <svg viewBox="0 0 24 24" role="img" aria-label={`${providerLabel[provider]} Logo`}><path fill={`#${icon.hex}`} d={icon.path} /></svg>;
  if (provider === 'outlook' || provider === 'hotmail') return <MicrosoftOutlookLogo weight="fill" aria-label="Microsoft Outlook Logo" />;
  if (provider === 'yahoo') return <span className="provider-yahoo-glyph" aria-label="Yahoo Logo">Y!</span>;
  return <EnvelopeSimple weight="duotone" aria-label="IMAP 邮箱" />;
}


export function AccountProviderMark({ provider, className = '' }: { provider: ProviderId; className?: string }) {
  return <span className={`account-provider-mark provider-${provider} ${className}`} aria-hidden="true"><ProviderIcon provider={provider} /></span>;
}

export function initials(value: string) {
  const parts = value.trim().split(/\s+/);
  return (parts.length > 1 ? parts.map((part) => part[0]).join('') : value.slice(0, 2)).toUpperCase();
}

export function SenderAvatar({ logo, name, color, large = false }: { logo: ContactLogo; name: string; color: string; large?: boolean }) {
  const [failed, setFailed] = useState(false);
  useEffect(() => { setFailed(false); }, [logo.url]);
  return <span className={`sender-avatar ${large ? 'large' : ''}`} style={{ '--avatar-color': color } as CSSProperties}>
    {initials(name)}
    {!failed && <img src={logo.url} alt="" loading="lazy" onError={() => setFailed(true)} />}
  </span>;
}

export function relativeTime(value: string) {
  const diff = Date.now() - new Date(value).getTime();
  if (diff < 60 * 60_000) return `${Math.max(1, Math.floor(diff / 60_000))} 分钟前`;
  if (diff < 24 * 60 * 60_000) return `${Math.floor(diff / 3600_000)} 小时前`;
  if (diff < 48 * 60 * 60_000) return '昨天';
  return new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric' }).format(new Date(value));
}

export function Overlay({ children, onClose, wide = false, dialogClassName = '' }: { children: ReactNode; onClose: () => void; wide?: boolean; dialogClassName?: string }) {
  const dialogRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);
  const titleId = useId();
  useEffect(() => { onCloseRef.current = onClose; }, [onClose]);
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    const focusable = () => Array.from(dialog?.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href]') ?? []);
    (focusable()[0] ?? dialog)?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); onCloseRef.current(); return; }
      if (event.key !== 'Tab') return;
      const items = focusable();
      if (items.length === 0) { event.preventDefault(); dialog?.focus(); return; }
      const first = items[0]; const last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => { document.removeEventListener('keydown', onKeyDown); previous?.focus(); };
  }, []);
  return <div className="overlay" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section ref={dialogRef} className={`modal ${wide ? 'modal-wide' : ''} ${dialogClassName}`.trim()} role="dialog" aria-modal="true" aria-labelledby={titleId} tabIndex={-1}><span id={titleId} className="sr-only">iMail 对话框</span>{children}</section>
  </div>;
}
