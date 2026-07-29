import type { Contact } from '../../types';

const emailPattern = /^[^\s@,]+@[^\s@,]+\.[^\s@,]+$/;

export function addressParts(value: string) {
  return value.split(',').map((item) => item.trim()).filter(Boolean);
}

export function validAddresses(value: string) {
  return addressParts(value).filter((address) => emailPattern.test(address));
}

export function invalidAddresses(value: string) {
  return addressParts(value).filter((address) => !emailPattern.test(address));
}

export function activeAddressQuery(value: string) {
  return (value.split(',').at(-1) ?? '').trim().replace(/^@/, '').toLocaleLowerCase();
}

export function matchingContacts(value: string, contacts: Contact[], limit = 8) {
  const query = activeAddressQuery(value);
  const selected = new Set(validAddresses(value).map((address) => address.toLocaleLowerCase()));
  return contacts.filter((contact) => {
    if (selected.has(contact.address.toLocaleLowerCase())) return false;
    return !query || contact.address.toLocaleLowerCase().includes(query) || contact.name.toLocaleLowerCase().includes(query);
  }).slice(0, limit);
}

export function selectContact(value: string, address: string) {
  const parts = value.split(',');
  parts[parts.length - 1] = ` ${address}`;
  return `${parts.map((part) => part.trim()).filter(Boolean).join(', ')}, `;
}
