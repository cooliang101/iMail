import type { RefObject } from 'preact/compat';
import { Bell, Info, MagnifyingGlass, SidebarSimple } from '../../components/icons';
import { AppInput } from '../../components/form-controls';
import { useI18n } from '../i18n';

export function AppTopbar({ sidebarCollapsed, sidebarOpen, search, searchPlaceholder = '搜索当前范围内的邮件', searchShortcut, searchInputRef, onToggleSidebar, onOpenMobileSidebar, onSearchChange, onNotifications, onAbout, onAdvancedSearch, advancedActive }: {
  onAdvancedSearch?: () => void; advancedActive?: boolean;
  sidebarCollapsed: boolean; sidebarOpen: boolean; search: string; searchPlaceholder?: string; searchShortcut: string; searchInputRef: RefObject<HTMLInputElement>;
  onToggleSidebar: () => void; onOpenMobileSidebar: () => void; onSearchChange: (value: string) => void; onNotifications: () => void; onAbout: () => void;
}) {
  const { t } = useI18n();
  const localizedPlaceholder = t(searchPlaceholder);
  return <header className="topbar">
    <button className="sidebar-trigger desktop-sidebar-trigger" title={t(sidebarCollapsed ? '展开侧栏' : '收起侧栏')} aria-label={t(sidebarCollapsed ? '展开侧栏' : '收起侧栏')} aria-expanded={!sidebarCollapsed} onClick={onToggleSidebar}><SidebarSimple size={20} /></button>
    <button className="sidebar-trigger mobile-sidebar-trigger" title={t('打开侧栏')} aria-label={t('打开侧栏')} aria-expanded={sidebarOpen} onClick={onOpenMobileSidebar}><SidebarSimple size={20} /></button>
    <AppInput className="search-box" contentBefore={<MagnifyingGlass size={18} />} contentAfter={<kbd>{searchShortcut}</kbd>} ref={searchInputRef} value={search} onChange={(event) => onSearchChange(event.currentTarget.value)} placeholder={localizedPlaceholder} aria-label={localizedPlaceholder} />
    {onAdvancedSearch && <button className="advanced-search-trigger" aria-label="高级搜索" aria-pressed={advancedActive} onClick={onAdvancedSearch}>高级</button>}
    <button data-icon-tone="neutral" className="icon-button" title={t('关于 iMail')} aria-label={t('打开关于 iMail')} onClick={onAbout}><Info size={19} /></button>
    <button data-icon-tone="info" className="icon-button" title={t('通知中心')} aria-label={t('打开通知中心')} onClick={onNotifications}><Bell size={19} /></button>
  </header>;
}
