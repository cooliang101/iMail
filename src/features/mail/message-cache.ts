import type { Message } from '../../types';

export function reconcileMessageCache(current: Message[], incoming: Message[]) {
  const currentById = new Map(current.map((message) => [message.id, message]));
  const reconciled = incoming.map((message) => {
    const cached = currentById.get(message.id);
    if (!cached) return message;
    const merged = cached.text !== undefined ? { ...message, text: cached.text, html: cached.html } : message;
    return JSON.stringify(cached) === JSON.stringify(merged) ? cached : merged;
  });
  return current.length === reconciled.length && current.every((message, index) => message === reconciled[index]) ? current : reconciled;
}
