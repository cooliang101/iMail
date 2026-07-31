import { describe, expect, it } from 'vitest';
import { addressParts, appendAddress, invalidAddresses, isValidAddress, matchingContacts, validAddresses } from './address-utils';

const contacts = [
  { address: 'alice@example.com', name: 'Alice Chen', messageCount: 4, lastContactAt: '2026-07-29T00:00:00.000Z', logo: { url: '/api/contacts/logo?address=alice%40example.com' } },
  { address: 'bob@example.com', name: 'Bob', messageCount: 2, lastContactAt: '2026-07-28T00:00:00.000Z', logo: { url: '/api/contacts/logo?address=bob%40example.com' } },
];

describe('compose address helpers', () => {
  it('recognizes space, tab, semicolon and comma as address separators', () => {
    const value = 'alice@example.com; bob@example.com\tcarol@example.com dave@example.com';
    expect(addressParts(value)).toEqual(['alice@example.com', 'bob@example.com', 'carol@example.com', 'dave@example.com']);
    expect(validAddresses(value)).toHaveLength(4);
    expect(invalidAddresses(`${value}; incomplete@`)).toEqual(['incomplete@']);
    expect(isValidAddress('alice@example.com')).toBe(true);
  });

  it('does not show contacts before input and filters after input', () => {
    expect(matchingContacts('', contacts)).toEqual([]);
    expect(matchingContacts('ali', contacts).map((contact) => contact.address)).toEqual(['alice@example.com']);
    expect(matchingContacts('Alice Chen', contacts).map((contact) => contact.address)).toEqual(['alice@example.com']);
    expect(matchingContacts('@', contacts)).toEqual(contacts);
  });

  it('excludes selected contacts and avoids duplicate tags case-insensitively', () => {
    const selected = ['alice@example.com'];
    expect(matchingContacts('example', contacts, ['alice@example.com']).map((contact) => contact.address)).toEqual(['bob@example.com']);
    expect(appendAddress(selected, 'ALICE@example.com')).toBe(selected);
    expect(appendAddress(['alice@example.com'], 'bob@example.com')).toEqual(['alice@example.com', 'bob@example.com']);
  });
});
