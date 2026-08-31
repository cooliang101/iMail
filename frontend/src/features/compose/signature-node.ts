import { Node, mergeAttributes } from '@tiptap/core';
import { textToHtml } from './compose-utils';

export const ComposeSignature = Node.create({
  name: 'composeSignature',
  group: 'block',
  content: 'block+',
  defining: true,
  addAttributes() {
    return { accountId: { default: '', parseHTML: element => element.getAttribute('data-compose-signature'), renderHTML: attributes => ({ 'data-compose-signature': attributes.accountId }) } };
  },
  parseHTML() { return [{ tag: 'div[data-compose-signature]' }]; },
  renderHTML({ HTMLAttributes }) { return ['div', mergeAttributes(HTMLAttributes), 0]; },
});

export function signatureHtml(accountId: string, text: string) {
  if (!text) return '';
  const safeId = accountId.replace(/[&<>"']/g, character => `&#${character.charCodeAt(0)};`);
  return `<div data-compose-signature="${safeId}">${textToHtml(text)}</div>`;
}
