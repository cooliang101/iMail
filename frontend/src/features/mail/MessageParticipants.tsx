import { useCallback, useEffect, useId, useRef, useState } from 'preact/compat';
import { SenderAvatar } from '../../components/shared';
import type { MailParticipant, ParticipantRole } from '../../app-model';
import type { Contact, Message } from '../../types';
import { ParticipantPopover } from './ParticipantPopover';

function participantLabel(participant: { name: string; address: string }) {
  return participant.name && participant.name !== participant.address
    ? `${participant.name} <${participant.address}>`
    : participant.address;
}

type ActiveParticipant = {
  role: ParticipantRole;
  participant: MailParticipant;
  contact?: Contact;
  trigger: HTMLButtonElement;
};

export function MessageParticipants({ message, contacts, color, onCompose, onFilter }: {
  message: Message;
  contacts: Contact[];
  color: string;
  onCompose: (address: string) => void;
  onFilter: (role: ParticipantRole, participant: MailParticipant) => void;
}) {
  const [active, setActive] = useState<ActiveParticipant | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const popoverId = useId();
  const contactByAddress = new Map(contacts.map((contact) => [contact.address.trim().toLocaleLowerCase(), contact]));
  const senderContact = contactByAddress.get(message.from.address.trim().toLocaleLowerCase());
  const sender = { name: senderContact?.name || message.from.name || message.from.address, address: message.from.address };
  const seenRecipients = new Set<string>();
  const recipients = message.to.filter((participant) => {
    const key = participant.address.trim().toLocaleLowerCase();
    if (!key || seenRecipients.has(key)) return false;
    seenRecipients.add(key);
    return true;
  });

  const close = useCallback((restoreFocus = false) => {
    setActive((current) => {
      if (restoreFocus) window.requestAnimationFrame(() => current?.trigger.focus());
      return null;
    });
  }, []);

  useEffect(() => {
    if (!active) return;
    const closeOnPointer = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!rootRef.current?.contains(target) && !document.getElementById(popoverId)?.contains(target)) close(true);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') close(true);
    };
    document.addEventListener('pointerdown', closeOnPointer);
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      document.removeEventListener('pointerdown', closeOnPointer);
      document.removeEventListener('keydown', closeOnEscape);
    };
  }, [active, close, popoverId]);

  useEffect(() => { setActive(null); }, [message.id]);

  function toggle(role: ParticipantRole, participant: MailParticipant, trigger: HTMLButtonElement) {
    if (active?.role === role && active.participant.address.toLocaleLowerCase() === participant.address.toLocaleLowerCase()) {
      close(true);
      return;
    }
    setActive({ role, participant, contact: contactByAddress.get(participant.address.trim().toLocaleLowerCase()), trigger });
  }

  return <div className="message-participants" ref={rootRef}>
    <button className="sender-contact-trigger" type="button" aria-label={`查看 ${sender.name} 的联系人名片`} aria-haspopup="dialog" aria-controls={active?.role === 'sender' ? popoverId : undefined} aria-expanded={active?.role === 'sender'} onClick={(event) => toggle('sender', sender, event.currentTarget)}>
      <SenderAvatar logo={senderContact?.logo ?? message.from.logo} name={sender.name} color={color} large />
      <span className="sender-copy">
        <strong>{sender.name}</strong>
        <small>{message.from.address}</small>
      </span>
    </button>
    <div className="recipient-line">
      <span>收件人</span>
      <span>{recipients.length > 0 ? recipients.map((recipient) => <button className="recipient-address" type="button" key={recipient.address.toLocaleLowerCase()} title={participantLabel(recipient)} aria-label={`查看收件人 ${participantLabel(recipient)}`} aria-haspopup="dialog" aria-controls={active?.role === 'recipient' && active.participant.address.toLocaleLowerCase() === recipient.address.toLocaleLowerCase() ? popoverId : undefined} aria-expanded={active?.role === 'recipient' && active.participant.address.toLocaleLowerCase() === recipient.address.toLocaleLowerCase()} onClick={(event) => toggle('recipient', recipient, event.currentTarget)}>{participantLabel(recipient)}</button>) : <span className="recipient-address-empty">未提供收件人信息</span>}</span>
    </div>
    {active && <ParticipantPopover id={popoverId} role={active.role} participant={active.participant} contact={active.contact} logo={active.role === 'sender' ? message.from.logo : undefined} color={color} trigger={active.trigger} onClose={close} onCompose={onCompose} onFilter={onFilter} />}
  </div>;
}
