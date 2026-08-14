import type { Message } from '../../types';

export type MessageMutation = Partial<Pick<Message, 'unread' | 'flagged' | 'mailbox' | 'mailboxRole'>>;

export function applyOptimisticMessageMutation(messages: Message[], messageId: string, mutation: MessageMutation) {
  return messages.map((message) => message.id === messageId ? { ...message, ...mutation } : message);
}

export function rollbackOptimisticMessageMutation(messages: Message[], messageId: string, optimistic: MessageMutation, previous: MessageMutation) {
  return messages.map((message) => {
    if (message.id !== messageId) return message;
    const fields = Object.keys(optimistic) as Array<keyof MessageMutation>;
    if (!fields.every((field) => message[field] === optimistic[field])) return message;
    return { ...message, ...previous };
  });
}

export class MessageActionCoordinator {
  private readonly active = new Set<string>();

  isActive(messageId: string) {
    return this.active.has(messageId);
  }

  begin(messageId: string) {
    if (this.active.has(messageId)) return false;
    this.active.add(messageId);
    return true;
  }

  end(messageId: string) {
    this.active.delete(messageId);
  }

  async run(messageId: string, operation: () => Promise<void>) {
    if (!this.begin(messageId)) return false;
    try {
      await operation();
      return true;
    } finally {
      this.end(messageId);
    }
  }
}
