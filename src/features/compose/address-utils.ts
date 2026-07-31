import type { Contact } from '../../types';

const emailPattern = /^[^\s@,;]+@[^\s@,;]+\.[^\s@,;]+$/;

export function isValidAddress(value: string) {
  return emailPattern.test(value.trim());
}

export function addressParts(value: string) {
  return value.split(/[,;\s]+/).map((item) => item.trim()).filter(Boolean);
}

export function validAddresses(value: string) {
  return addressParts(value).filter(isValidAddress);
}

export function invalidAddresses(value: string) {
  return addressParts(value).filter((address) => !isValidAddress(address));
}

export function matchingContacts(queryValue: string, contacts: Contact[], selectedAddresses: string[] = [], limit = 8) {
  const rawQuery = queryValue.trim().toLocaleLowerCase();
  if (!rawQuery) return [];
  const query = rawQuery.replace(/^@/, '');
  const selected = new Set(selectedAddresses.map((address) => address.toLocaleLowerCase()));
  return contacts.filter((contact) => {
    if (selected.has(contact.address.toLocaleLowerCase())) return false;
    return !query || contact.address.toLocaleLowerCase().includes(query) || contact.name.toLocaleLowerCase().includes(query);
  }).slice(0, limit);
}

export function appendAddress(addresses: string[], address: string) {
  const normalized = address.trim();
  return addresses.some((item) => item.toLocaleLowerCase() === normalized.toLocaleLowerCase()) ? addresses : [...addresses, normalized];
}
