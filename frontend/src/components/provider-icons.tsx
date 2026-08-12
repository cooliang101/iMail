import { EnvelopeSimple, MicrosoftOutlookLogo } from '@phosphor-icons/react';
import { siGmail, siIcloud, siQq } from 'simple-icons';
import type { ProviderId } from '../types';

export const providerLabel: Record<ProviderId, string> = { outlook: 'Outlook', gmail: 'Gmail', qq: 'QQ', yahoo: 'Yahoo', hotmail: 'Hotmail', icloud: 'iCloud', custom: 'IMAP' };
export const providers: Array<{ id: ProviderId; name: string; oauthKey?: 'google' | 'microsoft' | 'yahoo'; helpUrl?: string }> = [
  { id: 'outlook', name: 'Outlook', oauthKey: 'microsoft' }, { id: 'gmail', name: 'Gmail', oauthKey: 'google' },
  { id: 'qq', name: 'QQ 邮箱' }, { id: 'yahoo', name: 'Yahoo', oauthKey: 'yahoo' },
  { id: 'hotmail', name: 'Hotmail', oauthKey: 'microsoft' }, { id: 'icloud', name: 'iCloud' },
  { id: 'custom', name: '更多邮箱' },
];

const simpleProviderIcons: Partial<Record<ProviderId, { path: string; hex: string; title: string }>> = { gmail: siGmail, qq: siQq, icloud: siIcloud };

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
