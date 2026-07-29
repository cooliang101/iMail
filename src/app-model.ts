export type Notice = { kind: 'success' | 'error'; text: string } | null;

export type MailNotification = {
  id: string;
  kind: 'error' | 'snooze' | 'unread';
  title: string;
  detail: string;
  date: string;
  messageId?: string;
  accountId: string;
};

