import { describe, expect, it } from 'vitest';
import { clearParticipantFilter, EMPTY_PARTICIPANT_FILTERS, hasParticipantFilters, setParticipantFilter } from './participant-filter';

describe('participant filters', () => {
  it('replaces only the selected direction and normalizes its address', () => {
    const sender = setParticipantFilter(EMPTY_PARTICIPANT_FILTERS, 'sender', { name: ' Wayne ', address: ' sender@example.com ' });
    const combined = setParticipantFilter(sender, 'recipient', { name: '', address: 'alias@icloud.com' });
    const replaced = setParticipantFilter(combined, 'sender', { name: 'Other', address: 'other@example.com' });
    expect(replaced).toEqual({
      sender: { name: 'Other', address: 'other@example.com' },
      recipient: { name: '', address: 'alias@icloud.com' },
    });
    expect(clearParticipantFilter(replaced, 'recipient').recipient).toBeNull();
    expect(hasParticipantFilters(replaced)).toBe(true);
    expect(hasParticipantFilters(EMPTY_PARTICIPANT_FILTERS)).toBe(false);
  });
});
