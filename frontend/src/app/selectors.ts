import type { Account } from '../types';
import type { WorkspaceFolder } from '../app-model';
import { isWorkspaceMailbox } from '../features/organize';

export function buildWorkspaceFolders(accounts: Account[], groups: string[]) {
  return new Map(groups.map((group) => {
    const merged = new Map<string, WorkspaceFolder>();
    for (const account of accounts.filter((item) => item.group === group)) {
      for (const mailbox of account.mailboxes.filter(isWorkspaceMailbox)) {
        const key = mailbox.name.toLocaleLowerCase();
        const current = merged.get(key) ?? { group, name: mailbox.name, unread: 0, targets: [] };
        current.unread += mailbox.unread ?? 0;
        current.targets.push({ accountId: account.id, accountName: account.displayName, path: mailbox.path });
        merged.set(key, current);
      }
    }
    return [group, Array.from(merged.values()).sort((left, right) => left.name.localeCompare(right.name, 'zh-CN'))] as const;
  }));
}
