import { Folder } from '@phosphor-icons/react';

export type WorkspaceFolder = {
  group: string;
  name: string;
  unread: number;
  targets: Array<{ accountId: string; accountName: string; path: string }>;
};

export function WorkspaceFolderItem({ folder, active, onSelect }: {
  folder: WorkspaceFolder;
  active: boolean;
  onSelect: (folder: WorkspaceFolder) => void;
}) {
  const accountNames = Array.from(new Set(folder.targets.map((target) => target.accountName)));
  const firstAccount = accountNames[0] ?? '邮箱';

  return <button
    data-icon-tone="info"
    className={active ? 'active' : ''}
    onClick={() => onSelect(folder)}
    title={folder.targets.map((target) => `${target.accountName} · ${target.path}`).join('\n')}
  >
    <Folder size={15} />
    <span className="workspace-mailbox-label">
      <small className="workspace-account-tag">{firstAccount}</small>
      {accountNames.length > 1 && <small className="workspace-account-more">+{accountNames.length - 1}</small>}
      <span className="workspace-folder-name">{folder.name}</span>
    </span>
    <b>{folder.unread || ''}</b>
  </button>;
}
