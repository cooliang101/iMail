import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'preact/compat';
import { Check, Copy, EnvelopeSimple, MagnifyingGlass } from '../../components/icons';
import { SenderAvatar } from '../../components/shared';
import type { MailParticipant, ParticipantRole } from '../../app-model';
import type { Contact, ContactLogo } from '../../types';

function contactRecency(value?: string) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'short', day: 'numeric' }).format(date);
}

export function ParticipantPopover({ id, role, participant, contact, logo, color, trigger, onClose, onCompose, onFilter }: {
  id: string;
  role: ParticipantRole;
  participant: MailParticipant;
  contact?: Contact;
  logo?: ContactLogo;
  color: string;
  trigger: HTMLButtonElement;
  onClose: (restoreFocus?: boolean) => void;
  onCompose: (address: string) => void;
  onFilter: (role: ParticipantRole, participant: MailParticipant) => void;
}) {
  const popoverRef = useRef<HTMLElement>(null);
  const [copied, setCopied] = useState(false);
  const [position, setPosition] = useState({ left: 12, top: 12, width: 320 });

  useEffect(() => { setCopied(false); }, [participant.address]);
  useLayoutEffect(() => {
    let focusFrame = 0;
    const updatePosition = () => {
      const card = popoverRef.current;
      const anchor = trigger.getBoundingClientRect();
      const viewportPadding = 12;
      const width = Math.min(340, Math.max(0, window.innerWidth - viewportPadding * 2));
      const height = card?.offsetHeight ?? 170;
      const left = Math.min(Math.max(anchor.left, viewportPadding), Math.max(viewportPadding, window.innerWidth - width - viewportPadding));
      const below = anchor.bottom + 8;
      const top = below + height <= window.innerHeight - viewportPadding
        ? below
        : Math.max(viewportPadding, anchor.top - height - 8);
      setPosition({ left, top, width });
    };
    updatePosition();
    focusFrame = window.requestAnimationFrame(() => popoverRef.current?.querySelector<HTMLButtonElement>('button')?.focus());
    window.addEventListener('resize', updatePosition);
    document.addEventListener('scroll', updatePosition, true);
    return () => {
      window.cancelAnimationFrame(focusFrame);
      window.removeEventListener('resize', updatePosition);
      document.removeEventListener('scroll', updatePosition, true);
    };
  }, [trigger]);

  async function copyAddress() {
    try {
      await navigator.clipboard.writeText(participant.address);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      setCopied(false);
    }
  }

  const title = participant.name && participant.name !== participant.address ? participant.name : participant.address;
  return <section
    ref={popoverRef}
    id={id}
    className={`participant-popover ${role === 'sender' ? 'sender-contact-card' : 'recipient-contact-card'}`}
    role="dialog"
    aria-label={role === 'sender' ? `${title} 的联系人名片` : `${title} 的收件人信息`}
    style={{ '--participant-left': `${position.left}px`, '--participant-top': `${position.top}px`, '--participant-width': `${position.width}px` } as CSSProperties}
  >
    <header>
      <SenderAvatar logo={contact?.logo ?? logo} name={title} color={color} large />
      <span><strong>{title}</strong><small title={participant.address}>{participant.address}</small></span>
      <button type="button" className={copied ? 'participant-copy-button is-copied' : 'participant-copy-button'} aria-label={copied ? '邮箱已复制' : '复制邮箱'} title={copied ? '已复制' : '复制邮箱'} onClick={() => void copyAddress()}>
        {copied ? <Check size={16} weight="bold" /> : <Copy size={16} />}
      </button>
    </header>
    {role === 'sender' && contact && <div className="participant-contact-facts"><span><strong>{contact.messageCount}</strong> 封往来邮件</span>{contact.lastContactAt && <span>最近联系 {contactRecency(contact.lastContactAt)}</span>}</div>}
    <footer>
      <button type="button" title={role === 'sender' ? '查看来自此地址的邮件' : '查看发往此地址的邮件'} onClick={() => { onClose(false); onFilter(role, participant); }}><MagnifyingGlass size={16} />{role === 'sender' ? '来自此地址' : '发往此地址'}</button>
      <button type="button" className="participant-compose-action" onClick={() => { onClose(false); onCompose(participant.address); }}><EnvelopeSimple size={16} />写邮件</button>
    </footer>
  </section>;
}
