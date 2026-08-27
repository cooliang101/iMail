import type { MailParticipant, ParticipantFilters, ParticipantRole } from '../../app-model';

export const EMPTY_PARTICIPANT_FILTERS: ParticipantFilters = { sender: null, recipient: null };

export function normalizeParticipant(participant: MailParticipant): MailParticipant {
  return { name: participant.name.trim(), address: participant.address.trim() };
}

export function setParticipantFilter(filters: ParticipantFilters, role: ParticipantRole, participant: MailParticipant): ParticipantFilters {
  return { ...filters, [role]: normalizeParticipant(participant) };
}

export function clearParticipantFilter(filters: ParticipantFilters, role: ParticipantRole): ParticipantFilters {
  return { ...filters, [role]: null };
}

export function hasParticipantFilters(filters: ParticipantFilters) {
  return Boolean(filters.sender || filters.recipient);
}
