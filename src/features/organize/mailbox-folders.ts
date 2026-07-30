import type { MailboxFolder } from '../../types';

const primarySpecialUses = new Set([
  '\\inbox', '\\sent', '\\archive', '\\all', '\\drafts', '\\trash', '\\junk', '\\flagged', '\\important',
]);

const primaryFolderNames = new Set([
  'inbox',
  'sent', 'sent items', 'sent messages', 'sent mail',
  'archive', 'archives', 'all mail',
  'draft', 'drafts',
  'trash', 'bin', 'deleted', 'deleted item', 'deleted items', 'deleted message', 'deleted messages',
  'junk', 'spam', 'junk mail', 'junk email', 'bulk mail',
  'flagged', 'starred', 'important',
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
