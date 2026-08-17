import { Archive, ArrowBendUpLeft, ArrowBendUpRight, ArrowClockwise, Clock, Envelope, FolderOpen, Gear, Keyboard, PencilSimple, Star, Tag, Trash, Tray } from '../../components/icons';
import type { ContextTarget, ShortcutBindings, WorkspaceFolder } from '../../app-model';
import type { Account, Message } from '../../types';
import { ContextMenu, type ContextMenuItem } from '../../components/ContextMenu';
import { shortcutLabel } from '../shortcuts';

type Actions = {
  openMessage: (id: string) => void; reply: () => void; forward: () => void; toggleStar: (message: Message) => void; setUnread: (message: Message, unread: boolean) => void;
  snooze: () => void; labels: () => void; archive: (message: Message) => void; delete: (message: Message) => void;
  openAccount: (id: string) => void; compose: (accountId?: string) => void; syncAccount: (id: string) => void; accountSettings: () => void;
  openWorkspace: (group: string) => void; syncWorkspace: (group: string) => void; editWorkspace: (group: string) => void;
  openFolder: (folder: WorkspaceFolder) => void; syncFolder: (folder: WorkspaceFolder) => void; syncCurrent: () => void; shortcutSettings: () => void;
};

export function AppContextMenu({ target, bindings, messages, accounts, activeAccountId, actions, onClose }: { target: ContextTarget; bindings: ShortcutBindings; messages: Message[]; accounts: Account[]; activeAccountId?: string; actions: Actions; onClose: () => void }) {
  const shortcut = (id: keyof ShortcutBindings) => bindings[id] ? shortcutLabel(bindings[id]) : undefined;
  let label = '当前页面';
  let items: ContextMenuItem[] = [];

  if (target.kind === 'message') {
    label = '邮件操作'; const message = messages.find((item) => item.id === target.messageId); if (!message) return null;
    items = [
      { id: 'open', label: '打开邮件', icon: <FolderOpen size={17} />, onSelect: () => actions.openMessage(message.id) },
      { id: 'reply', label: '回复', icon: <ArrowBendUpLeft size={17} />, shortcut: shortcut('reply'), onSelect: actions.reply },
      { id: 'forward', label: '转发', icon: <ArrowBendUpRight size={17} />, shortcut: shortcut('forward'), onSelect: actions.forward },
      { id: 'star', label: message.flagged ? '取消星标' : '添加星标', icon: <Star size={17} weight={message.flagged ? 'fill' : 'regular'} />, shortcut: shortcut('toggleStar'), separatorBefore: true, onSelect: () => actions.toggleStar(message) },
      { id: 'read', label: message.unread ? '标记为已读' : '标记为未读', icon: <Envelope size={17} />, shortcut: message.unread ? undefined : shortcut('markUnread'), onSelect: () => actions.setUnread(message, !message.unread) },
      { id: 'snooze', label: '稍后处理', icon: <Clock size={17} />, onSelect: actions.snooze },
      { id: 'labels', label: '管理标签', icon: <Tag size={17} />, onSelect: actions.labels },
      { id: 'archive', label: '归档', icon: <Archive size={17} />, shortcut: shortcut('archive'), disabled: message.mailboxRole !== 'inbox', separatorBefore: true, onSelect: () => actions.archive(message) },
      { id: 'delete', label: '移到垃圾箱', icon: <Trash size={17} />, shortcut: shortcut('delete'), danger: true, onSelect: () => actions.delete(message) },
    ];
  } else if (target.kind === 'account') {
    label = '邮箱操作'; const account = accounts.find((item) => item.id === target.accountId); if (!account) return null;
    items = [
      { id: 'open', label: '打开此邮箱', icon: <Tray size={17} />, onSelect: () => actions.openAccount(account.id) },
      { id: 'compose', label: '用此邮箱写信', icon: <PencilSimple size={17} />, shortcut: shortcut('compose'), onSelect: () => actions.compose(account.id) },
      { id: 'sync', label: '同步此邮箱', icon: <ArrowClockwise size={17} />, onSelect: () => actions.syncAccount(account.id) },
      { id: 'settings', label: '邮箱设置', icon: <Gear size={17} />, separatorBefore: true, onSelect: actions.accountSettings },
    ];
  } else if (target.kind === 'workspace') {
    label = '工作空间操作'; items = [
      { id: 'open', label: '打开工作空间', icon: <Tray size={17} />, onSelect: () => actions.openWorkspace(target.group) },
      { id: 'sync', label: '同步工作空间', icon: <ArrowClockwise size={17} />, onSelect: () => actions.syncWorkspace(target.group) },
      { id: 'edit', label: '编辑工作空间', icon: <PencilSimple size={17} />, separatorBefore: true, onSelect: () => actions.editWorkspace(target.group) },
    ];
  } else if (target.kind === 'folder') {
    label = '文件夹操作'; items = [
      { id: 'open', label: '打开文件夹', icon: <FolderOpen size={17} />, onSelect: () => actions.openFolder(target.folder) },
      { id: 'sync', label: '同步文件夹', icon: <ArrowClockwise size={17} />, onSelect: () => actions.syncFolder(target.folder) },
    ];
  } else {
    items = [
      { id: 'compose', label: '写新邮件', icon: <PencilSimple size={17} />, shortcut: shortcut('compose'), onSelect: () => actions.compose(activeAccountId) },
      { id: 'sync', label: '同步当前范围', icon: <ArrowClockwise size={17} />, shortcut: shortcut('sync'), onSelect: actions.syncCurrent },
      { id: 'shortcuts', label: '快捷键设置', icon: <Keyboard size={17} />, shortcut: shortcut('openShortcutSettings'), separatorBefore: true, onSelect: actions.shortcutSettings },
    ];
  }

  return <ContextMenu x={target.x} y={target.y} label={label} items={items} onClose={onClose} />;
}
