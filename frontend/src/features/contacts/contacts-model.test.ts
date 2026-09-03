import { describe, expect, it } from 'vitest';
import type { Contact } from '../../types';
import { contactAccent, contactRecency, filterContacts } from './contacts-model';

const contacts: Contact[] = [
  { address: 'alice@example.com', name: 'Alice Chen', messageCount: 4, lastContactAt: '2026-08-05T02:00:00.000Z', logo: { url: '/alice' } },
  { address: 'bob@company.test', name: 'Bob', messageCount: 2, lastContactAt: '2026-08-03T02:00:00.000Z', logo: { url: '/bob' } },
];

describe('contacts workspace model', () => {
  it('filters by display name or address without changing the source order', () => {
    expect(filterContacts(contacts, 'chen')).toEqual([contacts[0]]);
    expect(filterContacts(contacts, 'company.test')).toEqual([contacts[1]]);
    expect(filterContacts(contacts, '')).toEqual(contacts);
  });

  it('keeps generated accents stable for an address', () => {
    expect(contactAccent('Alice@Example.com')).toBe(contactAccent('alice@example.com'));
  });

  it('formats recent contact dates in plain language', () => {
    const now = new Date('2026-08-05T12:00:00.000Z');
    expect(contactRecency('2026-08-05T02:00:00.000Z', now)).toBe('今天联系');
    expect(contactRecency('2026-08-03T02:00:00.000Z', now)).toBe('2 天前联系');
    expect(contactRecency('', now)).toBe('联系时间未知');
  });
});
