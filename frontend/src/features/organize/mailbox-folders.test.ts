import { describe, expect, it } from 'vitest';
import type { MailboxFolder } from '../../types';
import { isWorkspaceMailbox } from './mailbox-folders';

function folder(path: string, specialUse?: string): MailboxFolder {
  return { path, name: path.split('/').at(-1) ?? path, delimiter: '/', specialUse, selectable: true, subscribed: true };
}

describe('isWorkspaceMailbox', () => {
  it.each(['Drafts', 'Deleted', 'Deleted Message', 'Deleted Messages', 'Deleted Items', 'Junk', 'Junk E-mail', 'Spam', 'Bulk Mail', 'Sent Items', 'All Mail', 'Starred', '草稿箱', '已删除邮件', '垃圾邮件', '归档邮件', '已加星标', '重要'])(
    'keeps the common system folder %s out of workspaces',
    (path) => expect(isWorkspaceMailbox(folder(path))).toBe(false),
  );

  it('keeps RFC special-use and provider-important folders out of workspaces', () => {
    expect(isWorkspaceMailbox(folder('草稿', '\\Drafts'))).toBe(false);
    expect(isWorkspaceMailbox(folder('重要', '\\Important'))).toBe(false);
  });

  it('retains genuine custom folders and avoids partial-name matches', () => {
    expect(isWorkspaceMailbox(folder('Projects/Alpha'))).toBe(true);
    expect(isWorkspaceMailbox(folder('Project Drafts 2026'))).toBe(true);
  });
});
