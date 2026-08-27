import { useEffect, useRef, useState } from 'preact/compat';
import { Check, Copy, EnvelopeSimple } from '../../components/icons';
import { SenderAvatar } from '../../components/shared';
import type { Contact, Message } from '../../types';

function participantLabel(participant: { name: string; address: string }) {
  return participant.name && participant.name !== participant.address
    ? `${participant.name} <${participant.address}>`
    : participant.address;
}

function contactRecency(value?: string) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'short', day: 'numeric' }).format(date);
}

export function MessageParticipants({ message, contact, color, onCompose }: {
  message: Message;
  contact?: Contact;
  color: string;
  onCompose: () => void;
}) {
  const [cardOpen, setCardOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const senderName = contact?.name || message.from.name || message.from.address;
  const recipients = message.to.length > 0 ? message.to : [{ name: '', address: '未提供收件人信息' }];

  useEffect(() => {
    if (!cardOpen) return;
    const closeOnPointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setCardOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setCardOpen(false);
    };
    document.addEventListener('pointerdown', closeOnPointer);
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      document.removeEventListener('pointerdown', closeOnPointer);
      document.removeEventListener('keydown', closeOnEscape);
    };
  }, [cardOpen]);

  useEffect(() => { setCardOpen(false); setCopied(false); }, [message.id]);

  async function copyAddress() {
    await navigator.clipboard.writeText(message.from.address);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1800);
  }

  return <div className="message-participants" ref={rootRef}>
    <button className="sender-contact-trigger" type="button" aria-label={`查看 ${senderName} 的联系人名片`} aria-haspopup="dialog" aria-expanded={cardOpen} onClick={() => setCardOpen((current) => !current)}>
      <SenderAvatar logo={contact?.logo ?? message.from.logo} name={senderName} color={color} large />
      <span className="sender-copy">
        <strong>{senderName}</strong>
        <small>{message.from.address}</small>
      </span>
    </button>
    <div className="recipient-line">
      <span>收件人</span>
      <span>{recipients.map((recipient) => <span className="recipient-address" key={recipient.address} title={participantLabel(recipient)}>{participantLabel(recipient)}</span>)}</span>
    </div>
    {cardOpen && <section className="sender-contact-card" role="dialog" aria-label={`${senderName} 的联系人名片`}>
      <header>
        <SenderAvatar logo={contact?.logo ?? message.from.logo} name={senderName} color={color} large />
        <span><strong>{senderName}</strong><small>{message.from.address}</small></span>
      </header>
      {contact && <div className="sender-contact-facts"><span><strong>{contact.messageCount}</strong> 封往来邮件</span>{contact.lastContactAt && <span>最近联系 {contactRecency(contact.lastContactAt)}</span>}</div>}
      <footer>
        <button type="button" onClick={() => void copyAddress()}>{copied ? <Check size={16} /> : <Copy size={16} />}{copied ? '已复制' : '复制邮箱'}</button>
        <button type="button" className="sender-contact-primary" onClick={() => { setCardOpen(false); onCompose(); }}><EnvelopeSimple size={16} />写邮件</button>
      </footer>
    </section>}
  </div>;
}
