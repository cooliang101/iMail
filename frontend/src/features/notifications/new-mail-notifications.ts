import type { SystemNotification } from '../../platform/types';

type CreatedMailEvent = {
  id?: number;
  payload?: {
    message?: {
      id?: string;
      accountEmail?: string;
      mailboxRole?: string;
      from?: { name?: string; address?: string };
      subject?: string;
      preview?: string;
      unread?: boolean;
    };
  };
};

export type NewMailNotification = SystemNotification & { eventKey: string };

function compact(value: string | undefined, limit: number) {
  const normalized = value?.replace(/\s+/g, ' ').trim() ?? '';
  return normalized.length > limit ? `${normalized.slice(0, limit - 1)}…` : normalized;
}

export function newMailNotificationFromEvent(data: string): NewMailNotification | null {
  let event: CreatedMailEvent;
  try {
    event = JSON.parse(data) as CreatedMailEvent;
  } catch {
    return null;
  }
  const message = event.payload?.message;
  if (!message?.id || message.mailboxRole !== 'inbox' || message.unread === false) return null;

  const sender = compact(message.from?.name || message.from?.address, 48) || '未知发件人';
  const subject = compact(message.subject, 80) || '（无主题）';
  const preview = compact(message.preview, 120);
  const account = compact(message.accountEmail, 80);
  const detail = preview && preview !== subject ? `${subject}\n${preview}` : subject;

  return {
    eventKey: event.id === undefined ? message.id : String(event.id),
    tag: `imail-message-${message.id}`,
    title: `新邮件 · ${sender}`,
    body: account ? `${detail}\n${account}` : detail,
    target: { messageId: message.id, accountEmail: message.accountEmail },
  };
}
