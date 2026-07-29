import { describe, expect, it } from 'vitest';
import { invalidAddresses, matchingContacts, selectContact, validAddresses } from './address-utils';

const contacts = [
  { address: 'alice@example.com', name: 'Alice', messageCount: 4, lastContactAt: '2026-07-29T00:00:00.000Z' },
  { address: 'bob@example.com', name: 'Bob', messageCount: 2, lastContactAt: '2026-07-28T00:00:00.000Z' },
];

describe('compose address helpers', () => {
  it('does not submit an incomplete address while the user is typing', () => {
    expect(validAddresses('@')).toEqual([]);
    expect(validAddresses('alice@example.com, @')).toEqual(['alice@example.com']);
    expect(invalidAddresses('alice@example.com, @')).toEqual(['@']);
  });

  it('uses @ to show the contact library and replaces the active token', () => {
    expect(matchingContacts('@', contacts)).toEqual(contacts);
    expect(matchingContacts('ali', contacts).map((contact) => contact.address)).toEqual(['alice@example.com']);
    expect(selectContact('bob@example.com, @ali', 'alice@example.com')).toBe('bob@example.com, alice@example.com, ');
  });
});
