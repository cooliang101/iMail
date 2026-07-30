import { describe, expect, it } from 'vitest';
import { mailboxRoleFor } from './mailbox-role.js';

describe('mailboxRoleFor', () => {
  it.each([
    ['Drafts', undefined, 'drafts'],
    ['Deleted', undefined, 'trash'],
    ['Deleted Message', undefined, 'trash'],
    ['Deleted Messages', undefined, 'trash'],
    ['Deleted Items', undefined, 'trash'],
    ['Junk', undefined, 'junk'],
    ['Junk E-mail', undefined, 'junk'],
    ['Spam', undefined, 'junk'],
    ['Bulk Mail', undefined, 'junk'],
    ['Localized/Drafts', undefined, 'drafts'],
    ['任意名称', '\\Drafts', 'drafts'],
    ['任意名称', '\\Trash', 'trash'],
    ['任意名称', '\\Junk', 'junk'],
  ] as const)('maps %s (%s) to %s', (path, specialUse, role) => {
    expect(mailboxRoleFor({ path, specialUse })).toBe(role);
  });

  it('does not classify partial matches as system folders', () => {
    expect(mailboxRoleFor({ path: 'Project Drafts 2026' })).toBe('custom');
    expect(mailboxRoleFor({ path: 'Spam research' })).toBe('custom');
  });
});
