import { useMemo, useState, type KeyboardEvent } from 'react';
import type { Contact } from '../../types';
import { AppInput } from '../../components/form-controls';
import { SenderAvatar } from '../../components/shared';
import { matchingContacts, selectContact } from './address-utils';

export function AddressField({ label, value, contacts, placeholder, onChange }: {
  label: string;
  value: string;
  contacts: Contact[];
  placeholder: string;
  onChange: (value: string) => void;
}) {
  const [focused, setFocused] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const suggestions = useMemo(() => matchingContacts(value, contacts), [contacts, value]);
  const open = focused && suggestions.length > 0;

  function choose(address: string) {
    onChange(selectContact(value, address));
    setActiveIndex(0);
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (!open) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      setActiveIndex((current) => event.key === 'ArrowDown' ? (current + 1) % suggestions.length : (current - 1 + suggestions.length) % suggestions.length);
    } else if ((event.key === 'Enter' || event.key === 'Tab') && suggestions[activeIndex]) {
      event.preventDefault();
      choose(suggestions[activeIndex].address);
    } else if (event.key === 'Escape') {
      setFocused(false);
    }
  }

  return <div className="compose-row compose-address-row"><span>{label}</span><span className="compose-address-control">
    <AppInput type="text" inputMode="email" autoComplete="off" value={value} onChange={(event) => { onChange(event.target.value); setActiveIndex(0); }} onFocus={() => setFocused(true)} onBlur={() => window.setTimeout(() => setFocused(false), 120)} onKeyDown={onKeyDown} placeholder={placeholder} aria-label={label} aria-autocomplete="list" aria-expanded={open} />
    {open && <span className="contact-suggestions" role="listbox" aria-label={`${label}联系人`}>
      {suggestions.map((contact, index) => <button key={contact.address} type="button" role="option" aria-selected={index === activeIndex} className={index === activeIndex ? 'active' : ''} onMouseDown={(event) => event.preventDefault()} onClick={() => choose(contact.address)}>
        <SenderAvatar logo={contact.logo} name={contact.name || contact.address} color="#2b8a78" /><span><strong>{contact.name || contact.address}</strong>{contact.name && <small>{contact.address}</small>}</span><em>{contact.messageCount} 封往来</em>
      </button>)}
    </span>}
  </span></div>;
}
