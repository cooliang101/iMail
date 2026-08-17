import '../../styles/contacts.css';
import { AppButton } from '../../components/AppButton';
import { AddressBook, EnvelopeSimple, MagnifyingGlass } from '../../components/icons';
import type { Contact } from '../../types';
import { SenderAvatar } from '../../components/SenderAvatar';
import { contactAccent, contactRecency, filterContacts } from './contacts-model';

export function ContactsWorkspace({ contacts, search, onCompose }: { contacts: Contact[]; search: string; onCompose: (contact: Contact) => void }) {
  const visibleContacts = filterContacts(contacts, search);
  return <section className="contacts-workspace">
    <header className="contacts-heading">
      <div className="contacts-title-mark"><AddressBook size={25} weight="duotone" /></div>
      <div><span>通讯录</span><h1>联系人</h1><p>根据已同步邮件中的发件人与收件人自动整理</p></div>
      <strong>{contacts.length}<small>位联系人</small></strong>
    </header>
    {visibleContacts.length > 0 ? <div className="contacts-grid">
      {visibleContacts.map((contact) => <article className="contact-card" key={contact.address}>
        <SenderAvatar logo={contact.logo} name={contact.name || contact.address} color={contactAccent(contact.address)} large />
        <div className="contact-identity"><strong title={contact.name || contact.address}>{contact.name || contact.address}</strong><span title={contact.address}>{contact.address}</span></div>
        <div className="contact-meta"><span>{contact.messageCount} 封往来邮件</span><time dateTime={contact.lastContactAt}>{contactRecency(contact.lastContactAt)}</time></div>
        <AppButton appearance="subtle" icon={<EnvelopeSimple size={17} />} aria-label={`给 ${contact.name || contact.address} 写邮件`} onClick={() => onCompose(contact)}>写邮件</AppButton>
      </article>)}
    </div> : <div className="contacts-empty">
      {search.trim() ? <><MagnifyingGlass size={34} /><h2>没有匹配的联系人</h2><p>换个姓名或邮箱地址再试试。</p></> : <><AddressBook size={38} /><h2>联系人会自动出现在这里</h2><p>同步邮件后，发件人与收件人会被整理到通讯录。</p></>}
    </div>}
  </section>;
}
