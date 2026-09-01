import type { ComponentChildren } from 'preact';
import { AppButton } from '../../components/AppButton';
import { AddressBook, Archive, ArrowRight, ArrowsLeftRight, CaretDown, ClipboardText, Clock, Code, FolderSimplePlus, Gear, PaperPlaneTilt, PencilSimple, Plus, Star, Tag, Trash, Tray, WarningCircle, X } from '../../components/icons';
import type { Account } from '../../types';
import type { AppView, ContextTarget, MessageStats, WorkspaceFolder } from '../../app-model';
import { WorkspaceFolderItem, WorkspaceIcon } from '../organize';
import { AccountProviderMark } from '../../components/provider-icons';
import { useI18n } from '../i18n';

export function AppSidebar({ user, accounts, groups, workspaceFolders, labels, messageStats, draftsCount, outboxCount, workQueueCount, contactsCount, view, accountFilter, groupFilter, activeLabel, activeMailbox, expandedWorkspaces, sidebarOpen, onClose, onCompose, onAddAccount, onSettings, onSelectScope, onSelectMailbox, onSelectLabel, onEditWorkspace, onToggleWorkspace, onContextTarget, onLogout, smartFolders }: {
  smartFolders?: ComponentChildren;
  user: { login: string; displayName: string }; accounts: Account[]; groups: string[]; workspaceFolders: Map<string, WorkspaceFolder[]>; labels: string[]; messageStats: MessageStats; draftsCount: number; outboxCount: number; workQueueCount: number;
  contactsCount: number;
  view: AppView; accountFilter: string; groupFilter: string | null; activeLabel: string | null; activeMailbox: WorkspaceFolder | null; expandedWorkspaces: Set<string>; sidebarOpen: boolean;
  onClose: () => void; onCompose: (accountId?: string) => void; onAddAccount: () => void; onSettings: () => void; onSelectScope: (view: AppView, accountId?: string, group?: string | null) => void;
  onSelectMailbox: (folder: WorkspaceFolder) => void; onSelectLabel: (label: string) => void; onEditWorkspace: (group: string | null) => void; onToggleWorkspace: (group: string) => void;
  onContextTarget: (target: ContextTarget) => void; onLogout: () => void;
}) {
  const { t } = useI18n();
  const activeAccount = accountFilter === 'all' ? undefined : accounts.find((account) => account.id === accountFilter);
  return <aside className={`primary-sidebar ${sidebarOpen ? 'mobile-open' : ''}`}>
    <div className="sidebar-heading"><div><strong>iMail</strong><span>{t('统一通信工作台')}</span></div><button className="mobile-close" aria-label={t('关闭侧栏')} onClick={onClose}><X size={20} /></button></div>
    <AppButton appearance="primary" icon={<PencilSimple size={18} />} className="compose-button" title={t('写邮件')} aria-label={t('写邮件')} onClick={() => onCompose(activeAccount?.id)}><span className="compose-button-label">{t('写邮件')}</span></AppButton>
    <div className="mobile-account-controls" aria-label={t('移动端邮箱账户')}>
      <button className={accountFilter === 'all' ? 'active' : ''} onClick={() => onSelectScope('inbox')}><Tray size={18} /><span><strong>{t('全部邮箱')}</strong><small>{t('{count} 个账户', { count: accounts.length })}</small></span></button>
      {accounts.map((account) => <button key={account.id} className={accountFilter === account.id ? 'active' : ''} onClick={() => onSelectScope('inbox', account.id)}><AccountProviderMark provider={account.provider} /><span><strong>{account.displayName}</strong><small>{account.email}</small></span></button>)}
      <div><button onClick={onAddAccount}><Plus size={16} />{t('添加邮箱')}</button><button onClick={onSettings}><Gear size={16} />{t('设置')}</button></div>
      <button className="mobile-user-switch" type="button" onClick={onLogout}><ArrowsLeftRight size={17} /><span><strong>{user.displayName}</strong><small>{user.login} · {t('切换账号')}</small></span></button>
    </div>
    <nav className="nav-block">
      <button title={t('统一收件箱')} aria-label={t('统一收件箱')} data-icon-tone="primary" className={view === 'inbox' && !groupFilter ? 'active' : ''} onClick={() => onSelectScope('inbox')}><Tray size={19} /><span>{t('统一收件箱')}</span><b>{messageStats.unread || ''}</b></button>
      <button title={t('已加星标')} aria-label={t('已加星标')} data-icon-tone="warning" className={view === 'starred' ? 'active' : ''} onClick={() => onSelectScope('starred')}><Star size={19} /><span>{t('已加星标')}</span></button>
      <button title={t('已发送')} aria-label={t('已发送')} data-icon-tone="info" className={view === 'sent' ? 'active' : ''} onClick={() => onSelectScope('sent')}><PaperPlaneTilt size={19} /><span>{t('已发送')}</span></button>
      <button title={t('草稿')} aria-label={t('草稿')} data-icon-tone="accent" className={view === 'drafts' ? 'active' : ''} onClick={() => onSelectScope('drafts')}><PencilSimple size={19} /><span>{t('草稿')}</span><b>{draftsCount || ''}</b></button>
      <button title="发件箱" aria-label="发件箱" data-icon-tone="warning" className={view === 'outbox' ? 'active' : ''} onClick={() => onSelectScope('outbox')}><Clock size={19} /><span>发件箱</span><b>{outboxCount || ''}</b></button>
      <button title="处理队列" aria-label="邮件处理队列" data-icon-tone="primary" className={view === 'workQueue' ? 'active' : ''} onClick={() => onSelectScope('workQueue')}><ClipboardText size={19} /><span>处理队列</span><b>{workQueueCount || ''}</b></button>
      <button title={t('联系人')} aria-label={t('联系人')} data-icon-tone="info" className={view === 'contacts' ? 'active' : ''} onClick={() => onSelectScope('contacts')}><AddressBook size={19} /><span>{t('联系人')}</span><b>{contactsCount || ''}</b></button>
      <button title={t('稍后处理')} aria-label={t('稍后处理')} data-icon-tone="warning" className={view === 'snoozed' ? 'active' : ''} onClick={() => onSelectScope('snoozed')}><Clock size={19} /><span>{t('稍后处理')}</span></button>
      <button title={t('归档')} aria-label={t('归档')} data-icon-tone="neutral" className={view === 'archive' ? 'active' : ''} onClick={() => onSelectScope('archive')}><Archive size={19} /><span>{t('归档')}</span></button>
      <button title={t('已删除邮件')} aria-label={t('已删除邮件')} data-icon-tone="danger" className={view === 'trash' ? 'active' : ''} onClick={() => onSelectScope('trash')}><Trash size={19} /><span>{t('已删除邮件')}</span></button>
      <button title={t('垃圾邮件')} aria-label={t('垃圾邮件')} data-icon-tone="warning" className={view === 'junk' ? 'active' : ''} onClick={() => onSelectScope('junk')}><WarningCircle size={19} /><span>{t('垃圾邮件')}</span></button>
    </nav>
    {smartFolders}
    <section className="workspace-section"><div className="section-label"><span>{t('工作空间')}</span><button className="workspace-add" title={t('新增或整理工作空间')} aria-label={t('新增工作空间')} onClick={() => onEditWorkspace(null)}><FolderSimplePlus size={16} /></button></div>
      <nav className="nav-block groups workspace-list">{groups.map((group) => {
        const folders = workspaceFolders.get(group) ?? []; const expanded = expandedWorkspaces.has(group); const visibleFolders = expanded ? folders : folders.slice(0, 3);
        const icon = accounts.find((account) => account.group === group)?.groupIcon ?? 'folder';
        return <div className="workspace-group" key={group}>
          <div className="workspace-row"><button className={groupFilter === group ? 'active' : ''} onClick={() => onSelectScope('inbox', 'all', group)} onContextMenu={(event) => { event.preventDefault(); onContextTarget({ kind: 'workspace', group, x: event.clientX, y: event.clientY }); }}><WorkspaceIcon icon={icon} size={17} /><span>{group}</span><b>{messageStats.byGroup.find((item) => item.group === group)?.unread || ''}</b></button><button className="workspace-edit" title={`编辑工作空间 ${group}`} aria-label={`编辑工作空间 ${group}`} onClick={() => onEditWorkspace(group)}><PencilSimple size={14} /></button></div>
          <div className="workspace-mailboxes">{visibleFolders.map((folder) => <WorkspaceFolderItem key={folder.name.toLocaleLowerCase()} folder={folder} active={view === 'folder' && activeMailbox?.group === group && activeMailbox.name.toLocaleLowerCase() === folder.name.toLocaleLowerCase()} onSelect={onSelectMailbox} onContextMenu={(target, point) => onContextTarget({ kind: 'folder', folder: target, ...point })} />)}{folders.length > 3 && <button className={`workspace-folder-toggle ${expanded ? 'is-expanded' : ''}`} onClick={() => onToggleWorkspace(group)}><CaretDown size={14} /><span>{expanded ? t('收起') : t('更多 {count}', { count: folders.length - 3 })}</span></button>}</div>
        </div>;
      })}</nav>
    </section>
    {labels.length > 0 && <><div className="section-label"><span>{t('邮件标签')}</span></div><nav className="nav-block groups label-nav">{labels.map((label) => <button key={label} data-icon-tone="info" className={activeLabel === label ? 'active' : ''} onClick={() => onSelectLabel(label)}><Tag size={16} /><span>{label}</span></button>)}</nav></>}
    <div className="sidebar-spacer" />
    <button className={`developer-entry ${view === 'tokens' ? 'active' : ''}`} title={t('外部接入')} aria-label={t('外部接入')} onClick={() => onSelectScope('tokens')}><Code size={19} /><span><strong>{t('外部接入')}</strong><small>{t('MCP 与邮件 API')}</small></span><ArrowRight size={16} /></button>
  </aside>;
}
