import type { MailboxFolder } from '../../types';

const primarySpecialUses = new Set([
  '\\inbox', '\\sent', '\\archive', '\\all', '\\drafts', '\\trash', '\\junk', '\\flagged', '\\important',
]);

const primaryFolderNames = new Set([
  'inbox',
  '收件箱',
  'sent', 'sent items', 'sent messages', 'sent mail',
  '已发送', '已发送邮件', '发件箱',
  'archive', 'archives', 'all mail',
  '归档', '归档邮件', '所有邮件',
  'draft', 'drafts',
  '草稿', '草稿箱',
  'trash', 'bin', 'deleted', 'deleted item', 'deleted items', 'deleted message', 'deleted messages',
  '已删除', '已删除邮件', '已删除项目', '废纸篓', '回收站', '垃圾箱',
  'junk', 'spam', 'junk mail', 'junk email', 'bulk mail',
  '垃圾邮件',
  'flagged', 'starred', 'important',
  '星标', '星标邮件', '已加星标', '重要', '重要邮件',
]);

function normalizedLeaf(folder: Pick<MailboxFolder, 'path' | 'name'>) {
  const leaf = folder.name || folder.path.split(/[/.\\]/).at(-1) || folder.path;
  return leaf.trim().toLocaleLowerCase().replace(/\be[-_]?mail\b/g, 'email').replace(/[-_]+/g, ' ').replace(/\s+/g, ' ');
}

export function isWorkspaceMailbox(folder: MailboxFolder) {
  if (!folder.selectable) return false;
  if (primarySpecialUses.has(folder.specialUse?.toLocaleLowerCase() ?? '')) return false;
  return !primaryFolderNames.has(normalizedLeaf(folder));
}
