import type { Account, Message } from '../../types';

export function replyRecipients(original: Message | undefined, accounts: Account[], all: boolean) {
  if (!original) return { to: [], cc: [] };
  const seen = new Set(accounts.map(account => account.email.trim().toLowerCase()));
  const take = (addresses: Array<{ address: string }>) => addresses.flatMap(({ address }) => {
    const trimmed = address.trim();
    const key = trimmed.toLowerCase();
    if (!trimmed || seen.has(key)) return [];
    seen.add(key);
    return [trimmed];
  });
  const target = original.replyTo?.length ? original.replyTo : [original.from];
  const to = take(all ? [...target, ...original.to] : target);
  return { to, cc: all ? take(original.cc ?? []) : [] };
}

export function replyHeaders(original?: Message) {
  const valid = (id: string) => /^(?:<[^\s<>"\\]+@[^\s<>"\\]+>|[^\s<>"\\]+@[^\s<>"\\]+)$/.test(id) && id.length <= 998;
  const normalize = (id: string) => id.startsWith('<') ? id : `<${id}>`;
  const parent = original?.messageId && valid(original.messageId) ? normalize(original.messageId) : undefined;
  const previous = original?.references?.length ? original.references : original?.inReplyTo ?? [];
  const references = [...new Set([...previous.filter(valid).map(normalize), ...(parent ? [parent] : [])])].slice(-100);
  return { inReplyTo: parent ? [parent] : [], references };
}

/** Ignore quoted history and signatures when checking newly authored text. */
export function needsAttachmentReminder(html: string, attachmentCount: number, Parser: typeof DOMParser = DOMParser) {
  if (attachmentCount > 0) return false;
  const document = new Parser().parseFromString(`<!doctype html><html><body>${html}</body></html>`, 'text/html');
  document.querySelectorAll('blockquote, [data-compose-signature]').forEach(node => node.remove());
  const text = document.body.textContent ?? '';
  return /附件|随信附|附上|\battach(?:ed|ment|ments|ing)?\b/i.test(text);
}
