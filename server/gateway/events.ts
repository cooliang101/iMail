import type { CachedMessage, MailAccount } from '../types.js';
import { gatewayMessageSummary } from './presenters.js';

export type GatewayMessageCreatedEvent = {
  id: string;
  type: 'message.created';
  occurredAt: string;
  accountId: string;
  data: { message: ReturnType<typeof gatewayMessageSummary> };
};

type GatewayEvent = GatewayMessageCreatedEvent;
type GatewayEventListener = (event: GatewayEvent) => void;

class GatewayEventBus {
  private readonly listeners = new Set<GatewayEventListener>();

  subscribe(listener: GatewayEventListener) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  publishMessageCreated(account: MailAccount, messages: CachedMessage[]) {
    const occurredAt = new Date().toISOString();
    for (const message of messages) {
      const event: GatewayMessageCreatedEvent = {
        id: crypto.randomUUID(),
        type: 'message.created',
        occurredAt,
        accountId: account.id,
        data: { message: gatewayMessageSummary(message, account.email) },
      };
      for (const listener of this.listeners) {
        try { listener(event); }
        catch { /* Event delivery must never roll back a successful mailbox sync. */ }
      }
    }
  }
}

export const gatewayEvents = new GatewayEventBus();
