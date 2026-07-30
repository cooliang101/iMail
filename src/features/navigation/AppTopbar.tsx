import type { RefObject } from 'react';
import { ArrowClockwise, Bell, MagnifyingGlass, SidebarSimple } from '@phosphor-icons/react';
import { AppInput } from '../../components/form-controls';

export function AppTopbar({ sidebarCollapsed, sidebarOpen, search, searchShortcut, syncing, searchInputRef, onToggleSidebar, onOpenMobileSidebar, onSearchChange, onSync, onNotifications }: {
  sidebarCollapsed: boolean; sidebarOpen: boolean; search: string; searchShortcut: string; syncing: boolean; searchInputRef: RefObject<HTMLInputElement | null>;
  onToggleSidebar: () => void; onOpenMobileSidebar: () => void; onSearchChange: (value: string) => void; onSync: () => void; onNotifications: () => void;
}) {
  return <header className="topbar">
    <button className="sidebar-trigger desktop-sidebar-trigger" title={sidebarCollapsed ? '展开侧栏' : '收起侧栏'} aria-label={sidebarCollapsed ? '展开侧栏' : '收起侧栏'} aria-expanded={!sidebarCollapsed} onClick={onToggleSidebar}><SidebarSimple size={20} /></button>
    <button className="sidebar-trigger mobile-sidebar-trigger" title="打开侧栏" aria-label="打开侧栏" aria-expanded={sidebarOpen} onClick={onOpenMobileSidebar}><SidebarSimple size={20} /></button>
    <AppInput className="search-box" contentBefore={<MagnifyingGlass size={18} />} contentAfter={<kbd>{searchShortcut}</kbd>} ref={searchInputRef} value={search} onChange={(event) => onSearchChange(event.target.value)} placeholder="搜索当前范围内的邮件" aria-label="搜索当前范围内的邮件" />
    <button data-icon-tone="primary" className={`sync-button ${syncing ? 'is-syncing' : ''}`} title="立即同步当前范围" onClick={onSync}><ArrowClockwise size={18} /><span>{syncing ? '已入队' : '立即同步'}</span></button>
    <button data-icon-tone="info" className="icon-button" title="通知中心" aria-label="打开通知中心" onClick={onNotifications}><Bell size={19} /></button>
  </header>;
}
