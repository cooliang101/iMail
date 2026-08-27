import { X } from '../../components/icons';
import type { MailParticipant, ParticipantFilters, ParticipantRole } from '../../app-model';

export function ParticipantFilterBar({ filters, onClear, onClearAll }: {
  filters: ParticipantFilters;
  onClear: (role: ParticipantRole) => void;
  onClearAll: () => void;
}) {
  const entries: Array<{ role: ParticipantRole; participant: MailParticipant }> = [];
  if (filters.sender) entries.push({ role: 'sender', participant: filters.sender });
  if (filters.recipient) entries.push({ role: 'recipient', participant: filters.recipient });
  if (entries.length === 0) return null;

  return <section className="participant-filter-bar" aria-label="邮件参与者筛选条件">
    {entries.map(({ role, participant }) => {
      const prefix = role === 'sender' ? '来自' : '发往';
      const name = participant.name && participant.name !== participant.address ? participant.name : '';
      return <span className="participant-filter-item" key={role} title={`${prefix} ${name ? `${name} · ` : ''}${participant.address}`}>
        <small>{prefix}</small>
        <span><strong>{name || participant.address}</strong>{name && <em>{participant.address}</em>}</span>
        <button type="button" aria-label={`清除${prefix}${participant.address}的筛选`} title="清除此条件" onClick={() => onClear(role)}><X size={13} /></button>
      </span>;
    })}
    {entries.length > 1 && <button type="button" className="participant-filter-clear-all" onClick={onClearAll}>清除全部</button>}
  </section>;
}
