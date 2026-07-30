import { getDomain } from 'tldts';
import type { ContactLogo, MailContact, StoreData } from './types.js';

export function contactDomain(address: string) {
  const hostname = address.split('@').at(-1)?.trim().toLowerCase().replace(/\.$/, '');
  if (!hostname || !/^[a-z0-9.-]+$/i.test(hostname)) return null;
  return { hostname, registrable: getDomain(hostname) ?? hostname };
}

export function contactLogoKey(address: string) {
  const domain = contactDomain(address);
  return domain ? `domain:${domain.hostname}` : null;
}

export function contactRootLogoKey(address: string) {
  const domain = contactDomain(address);
  return domain ? `domain:${domain.registrable}` : null;
}

export function contactLogoKeys(address: string) {
  const exact = contactLogoKey(address);
  const root = contactRootLogoKey(address);
  return exact && root ? { exact, root } : null;
}

export function reconcileContacts(data: Pick<StoreData, 'accounts' | 'messages' | 'contacts'>): MailContact[] {
  const ownAddresses = new Set(data.accounts.map((account) => account.email.trim().toLocaleLowerCase()));
  const previousByAddress = new Map((data.contacts ?? []).map((contact) => [contact.address.toLocaleLowerCase(), contact]));
  const logoByKey = new Map<string, ContactLogo>();
  for (const contact of data.contacts ?? []) if (contact.logo) logoByKey.set(contact.logo.key, contact.logo);
  const contacts = new Map<string, MailContact>();

  for (const message of data.messages) {
    const seenInMessage = new Set<string>();
    for (const participant of [message.from, ...message.to]) {
      const address = participant.address.trim();
      const key = address.toLocaleLowerCase();
      if (!address || ownAddresses.has(key) || seenInMessage.has(key)) continue;
      seenInMessage.add(key);
      const current = contacts.get(key);
      const previous = previousByAddress.get(key);
      const isLatest = !current || message.date > current.lastContactAt;
      const logoKeys = contactLogoKeys(address);
      const priorLogo = current?.logo ?? previous?.logo;
      contacts.set(key, {
        address: isLatest ? address : current.address,
        name: isLatest ? (participant.name.trim() || current?.name || previous?.name || '') : current.name,
        messageCount: (current?.messageCount ?? 0) + 1,
        lastContactAt: current && current.lastContactAt > message.date ? current.lastContactAt : message.date,
        logo: (logoKeys ? logoByKey.get(logoKeys.exact) ?? (priorLogo?.key === logoKeys.exact ? priorLogo : undefined)
          ?? logoByKey.get(logoKeys.root) : undefined) ?? priorLogo,
      });
    }
  }
  return Array.from(contacts.values()).sort((left, right) => right.lastContactAt.localeCompare(left.lastContactAt) || right.messageCount - left.messageCount || left.address.localeCompare(right.address));
}
