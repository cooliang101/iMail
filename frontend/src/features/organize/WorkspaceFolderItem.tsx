import { Folder } from '@phosphor-icons/react';
import type { WorkspaceFolder } from '../../app-model';

export function WorkspaceFolderItem({ folder, active, onSelect, onContextMenu }: {
  folder: WorkspaceFolder;
  active: boolean;
  onSelect: (folder: WorkspaceFolder) => void;
  onContextMenu?: (folder: WorkspaceFolder, point: { x: number; y: number }) => void;
}) {
  const accountNames = Array.from(new Set(folder.targets.map((target) => target.accountName)));
  const firstAccount = accountNames[0] ?? '邮箱';

  return <button
    data-icon-tone="info"
    className={active ? 'active' : ''}
    onClick={() => onSelect(folder)}
    onContextMenu={(event) => { event.preventDefault(); onContextMenu?.(folder, { x: event.clientX, y: event.clientY }); }}
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
