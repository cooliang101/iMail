import type { MessageStats } from '../../app-model';
import { accountStatsFromResponse, groupStatsFromResponse, messagesFromResponse, responseBoolean, responseNumber, responseOptionalString } from '../../services';
import type { Message } from '../../types';

export type MessagePage = { messages: Message[]; total: number; nextOffset: number; nextCursor?: string; hasMore: boolean };

export function messagePageFromResponse(value: unknown): MessagePage {
  return {
    messages: messagesFromResponse(value),
    total: responseNumber(value, 'total'),
    nextOffset: responseNumber(value, 'nextOffset'),
    nextCursor: responseOptionalString(value, 'nextCursor'),
    hasMore: responseBoolean(value, 'hasMore'),
  };
}

export function messageStatsFromResponse(value: unknown): MessageStats {
  return {
    total: responseNumber(value, 'total'),
    unread: responseNumber(value, 'unread'),
    byAccount: accountStatsFromResponse(value),
    byGroup: groupStatsFromResponse(value),
  };
}
