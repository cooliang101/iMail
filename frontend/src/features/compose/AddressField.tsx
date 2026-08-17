import { forwardRef, useImperativeHandle, useMemo, useRef, useState, type KeyboardEvent } from 'preact/compat';
import { X } from '../../components/icons';
import type { Contact } from '../../types';
import { AppInput } from '../../components/form-controls';
import { SenderAvatar } from '../../components/shared';
import { appendAddress, isValidAddress, matchingContacts } from './address-utils';

export type AddressFieldHandle = {
  resolve: () => { addresses: string[]; invalid?: string };
};

export const AddressField = forwardRef<AddressFieldHandle, {
  label: string;
  value: string[];
  contacts: Contact[];
  placeholder: string;
  onChange: (value: string[]) => void;
}>(function AddressField({ label, value, contacts, placeholder, onChange }, ref) {
  const [query, setQuery] = useState('');
  const [focused, setFocused] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const [invalid, setInvalid] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const suggestions = useMemo(() => matchingContacts(query, contacts, value), [contacts, query, value]);
  const open = focused && query.trim().length > 0 && suggestions.length > 0;

  function commit(address = query) {
    const candidate = address.trim();
    if (!candidate) return { addresses: value };
    if (!isValidAddress(candidate)) {
      setInvalid(true);
      return { addresses: value, invalid: candidate };
    }
    const addresses = appendAddress(value, candidate);
    if (addresses !== value) onChange(addresses);
    setQuery('');
    setInvalid(false);
    setActiveIndex(0);
    return { addresses };
  }

  function choose(address: string) {
    commit(address);
    inputRef.current?.focus();
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      if (!open) return;
      event.preventDefault();
      setActiveIndex((current) => event.key === 'ArrowDown' ? (current + 1) % suggestions.length : (current - 1 + suggestions.length) % suggestions.length);
      return;
    }
    if (event.key === 'Enter' && open && suggestions[activeIndex]) {
      event.preventDefault();
      choose(suggestions[activeIndex].address);
      return;
    }
    if (event.key === 'Enter' || event.key === 'Tab' || event.key === ';' || event.key === ',' || event.key === ' ') {
      if (!query.trim()) return;
      event.preventDefault();
      commit();
      return;
    }
    if (event.key === 'Backspace' && !query && value.length > 0) {
      const previous = value.at(-1) ?? '';
      onChange(value.slice(0, -1));
      setQuery(previous);
    } else if (event.key === 'Escape') {
      setFocused(false);
    }
  }

  useImperativeHandle(ref, () => ({ resolve: () => commit() }));

  return <div className="compose-row compose-address-row"><span>{label}</span><span className={`compose-address-control${invalid ? ' is-invalid' : ''}`}>
    <span className="compose-address-entry" onClick={() => inputRef.current?.focus()}>
      {value.map((address) => {
        const contact = contacts.find((item) => item.address.toLocaleLowerCase() === address.toLocaleLowerCase());
        return <span className="compose-address-tag" key={address} title={address}>
          <SenderAvatar logo={contact?.logo} name={contact?.name || address} color="#2b8a78" />
          <span>{contact?.name || address}</span>
          <button type="button" aria-label={`移除 ${address}`} onClick={(event) => { event.stopPropagation(); onChange(value.filter((item) => item !== address)); }}><X size={12} /></button>
        </span>;
      })}
      <AppInput ref={inputRef} type="text" inputMode="email" autoComplete="off" value={query} onChange={(event) => { setQuery(event.currentTarget.value); setInvalid(false); setActiveIndex(0); }} onFocus={() => setFocused(true)} onBlur={() => window.setTimeout(() => { setFocused(false); if (query.trim()) commit(); }, 120)} onKeyDown={onKeyDown} placeholder={value.length === 0 ? placeholder : '继续添加'} aria-label={label} aria-autocomplete="list" aria-expanded={open} aria-invalid={invalid} />
    </span>
    {invalid && <small className="compose-address-error">邮箱格式不正确，修改后按空格、Tab 或分号确认</small>}
    {open && <span className="contact-suggestions" role="listbox" aria-label={`${label}联系人`}>
      {suggestions.map((contact, index) => <button key={contact.address} type="button" role="option" aria-selected={index === activeIndex} className={index === activeIndex ? 'active' : ''} onMouseDown={(event) => event.preventDefault()} onClick={() => choose(contact.address)}>
        <SenderAvatar logo={contact.logo} name={contact.name || contact.address} color="#2b8a78" /><span><strong>{contact.name || contact.address}</strong>{contact.name && <small>{contact.address}</small>}</span><em>{contact.messageCount} 封往来</em>
      </button>)}
    </span>}
  </span></div>;
});
