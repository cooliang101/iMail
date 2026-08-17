import type { RefObject } from 'preact/compat';
import { Bell, MagnifyingGlass, SidebarSimple } from '../../components/icons';
import { AppInput } from '../../components/form-controls';

export function AppTopbar({ sidebarCollapsed, sidebarOpen, search, searchPlaceholder = '搜索当前范围内的邮件', searchShortcut, searchInputRef, onToggleSidebar, onOpenMobileSidebar, onSearchChange, onNotifications }: {
  sidebarCollapsed: boolean; sidebarOpen: boolean; search: string; searchPlaceholder?: string; searchShortcut: string; searchInputRef: RefObject<HTMLInputElement>;
  onToggleSidebar: () => void; onOpenMobileSidebar: () => void; onSearchChange: (value: string) => void; onNotifications: () => void;
}) {
  return <header className="topbar">
    <button className="sidebar-trigger desktop-sidebar-trigger" title={sidebarCollapsed ? '展开侧栏' : '收起侧栏'} aria-label={sidebarCollapsed ? '展开侧栏' : '收起侧栏'} aria-expanded={!sidebarCollapsed} onClick={onToggleSidebar}><SidebarSimple size={20} /></button>
    <button className="sidebar-trigger mobile-sidebar-trigger" title="打开侧栏" aria-label="打开侧栏" aria-expanded={sidebarOpen} onClick={onOpenMobileSidebar}><SidebarSimple size={20} /></button>
    <AppInput className="search-box" contentBefore={<MagnifyingGlass size={18} />} contentAfter={<kbd>{searchShortcut}</kbd>} ref={searchInputRef} value={search} onChange={(event) => onSearchChange(event.currentTarget.value)} placeholder={searchPlaceholder} aria-label={searchPlaceholder} />
    <button data-icon-tone="info" className="icon-button" title="通知中心" aria-label="打开通知中心" onClick={onNotifications}><Bell size={19} /></button>
  </header>;
}
