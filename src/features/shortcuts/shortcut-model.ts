import type { ShortcutActionId, ShortcutBindings } from '../../app-model';

export const shortcutStorageKey = 'imail.shortcut-bindings.v1';
export function shortcutStorageKeyFor(userId: string) { return `${shortcutStorageKey}:${userId}`; }

export const shortcutDefinitions: Array<{ id: ShortcutActionId; label: string; description: string; scope: 'global' | 'mail' }> = [
  { id: 'focusSearch', label: '搜索邮件', description: '聚焦并选中搜索框', scope: 'global' },
  { id: 'compose', label: '写新邮件', description: '打开新邮件编辑器', scope: 'global' },
  { id: 'sync', label: '同步当前范围', description: '同步当前邮箱或文件夹', scope: 'global' },
  { id: 'nextMessage', label: '下一封邮件', description: '在列表中向右切换', scope: 'mail' },
  { id: 'previousMessage', label: '上一封邮件', description: '在列表中向左切换', scope: 'mail' },
  { id: 'reply', label: '回复', description: '回复当前邮件', scope: 'mail' },
  { id: 'forward', label: '转发', description: '转发当前邮件', scope: 'mail' },
  { id: 'toggleStar', label: '切换星标', description: '添加或取消当前邮件星标', scope: 'mail' },
  { id: 'markUnread', label: '标记未读', description: '将当前邮件标记为未读', scope: 'mail' },
  { id: 'archive', label: '归档', description: '归档当前收件箱邮件', scope: 'mail' },
  { id: 'delete', label: '删除', description: '将当前邮件移到垃圾箱', scope: 'mail' },
  { id: 'openShortcutSettings', label: '快捷键设置', description: '打开快捷键编辑窗口', scope: 'global' },
];

export const defaultShortcutBindings: ShortcutBindings = {
  focusSearch: 'Mod+K', compose: 'C', sync: 'Mod+Shift+R', nextMessage: 'ArrowRight', previousMessage: 'ArrowLeft', reply: 'R', forward: 'F',
  toggleStar: 'S', markUnread: 'U', archive: 'A', delete: 'Delete', openShortcutSettings: 'Mod+/',
};

type KeyboardLike = Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>;

export function shortcutFromEvent(event: KeyboardLike) {
  const rawKey = event.key;
  if (['Control', 'Meta', 'Alt', 'Shift'].includes(rawKey)) return '';
  const key = rawKey === ' ' ? 'Space' : rawKey.length === 1 ? rawKey.toLocaleUpperCase() : rawKey;
  return [event.ctrlKey || event.metaKey ? 'Mod' : '', event.altKey ? 'Alt' : '', event.shiftKey ? 'Shift' : '', key].filter(Boolean).join('+');
}

export function shortcutMatches(event: KeyboardLike, binding: string) {
  return Boolean(binding) && shortcutFromEvent(event) === binding;
}

export function isBrowserRefreshShortcut(event: KeyboardLike) {
  return (event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey && event.key.toLocaleLowerCase() === 'r';
}

export function shortcutLabel(binding: string) {
  if (!binding) return '未设置';
  const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);
  return binding.split('+').map((part) => part === 'Mod' ? (isMac ? '⌘' : 'Ctrl') : part === 'Shift' ? '⇧' : part === 'Alt' ? (isMac ? '⌥' : 'Alt') : part === 'ArrowRight' ? '→' : part === 'ArrowLeft' ? '←' : part === 'Delete' ? (isMac ? '⌫' : 'Del') : part).join(isMac ? '' : ' + ');
}

export function loadShortcutBindings(storage: Pick<Storage, 'getItem'> = localStorage, key = shortcutStorageKey): ShortcutBindings {
  try {
    const saved = JSON.parse(storage.getItem(key) ?? '{}') as Partial<Record<ShortcutActionId, unknown>>;
    return Object.fromEntries(shortcutDefinitions.map(({ id }) => {
      const binding = typeof saved[id] === 'string' ? saved[id] : defaultShortcutBindings[id];
      return [id, id === 'sync' && binding === 'Mod+R' ? defaultShortcutBindings.sync : binding];
    })) as ShortcutBindings;
  } catch {
    return { ...defaultShortcutBindings };
  }
}

export function shortcutConflict(bindings: ShortcutBindings, actionId: ShortcutActionId, candidate: string) {
  return shortcutDefinitions.find(({ id }) => id !== actionId && bindings[id] === candidate);
}

export function isEditableShortcutTarget(target: EventTarget | null) {
  return target instanceof HTMLElement && (target.isContentEditable || Boolean(target.closest('input, textarea, select, [contenteditable="true"]')));
}
