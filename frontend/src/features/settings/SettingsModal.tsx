import { useState } from 'preact/compat';
import '../../styles/dialogs.css';
import '../../styles/settings.css';
import { ArrowCounterClockwise, Bell, Cloud, Envelope, Eye, Gear, HardDrives, Info, Keyboard, LockKey, Palette, X } from '../../components/icons';
import type { Account } from '../../types';
import type { AppPreferences, Notice, ShortcutBindings } from '../../app-model';
import { Overlay } from '../../components/shared';
import { AccountSettingsPanel } from '../accounts';
import { defaultShortcutBindings } from '../shortcuts';
import { DisplayPanel } from './DisplayPanel';
import { AppearancePanel } from './AppearancePanel';
import { GeneralPanel } from './GeneralPanel';
import { NotificationPanel } from './NotificationPanel';
import { PrivacyPanel } from './PrivacyPanel';
import { ShortcutPanel } from './ShortcutPanel';
import { ServicePanel } from '../service';
import { AppleHmeSettingsPanel } from './AppleHmeSettingsPanel';
import { AboutPanel } from './AboutPanel';

export type SettingsTab = 'general' | 'service' | 'appearance' | 'accounts' | 'apple-hme' | 'shortcuts' | 'notifications' | 'display' | 'privacy' | 'about';

const tabs: Array<{ id: SettingsTab; label: string; detail: string; icon: typeof Gear }> = [
  { id: 'general', label: '通用', detail: '启动与阅读行为', icon: Gear },
  { id: 'service', label: '服务连接', detail: '本地或远程服务', icon: HardDrives },
  { id: 'appearance', label: '主题', detail: '界面风格与色彩', icon: Palette },
  { id: 'accounts', label: '邮箱管理', detail: '授权、代理与工作空间', icon: Envelope },
  { id: 'apple-hme', label: '隐私邮箱', detail: 'iCloud Hide My Email', icon: Cloud },
  { id: 'shortcuts', label: '快捷键', detail: '键盘操作与绑定', icon: Keyboard },
  { id: 'notifications', label: '通知', detail: '选择需要关注的动态', icon: Bell },
  { id: 'display', label: '邮件展示', detail: '正文默认查看方式', icon: Eye },
  { id: 'privacy', label: '隐私与数据', detail: '本地优先与安全边界', icon: LockKey },
  { id: 'about', label: '关于 iMail', detail: '版本与项目主页', icon: Info },
];

export function SettingsModal({ initialTab, accounts, preferences, bindings, onPreferencesChange, onBindingsChange, onAddAccount, onReload, setNotice, onClose }: {
  initialTab: SettingsTab;
  accounts: Account[];
  preferences: AppPreferences;
  bindings: ShortcutBindings;
  onPreferencesChange: (preferences: AppPreferences) => void;
  onBindingsChange: (bindings: ShortcutBindings) => void;
  onAddAccount: () => void;
  onReload: () => Promise<void>;
  setNotice: (notice: Notice) => void;
  onClose: () => void;
}) {
  const [activeTab, setActiveTab] = useState(initialTab);
  return <Overlay onClose={onClose} wide dialogClassName="settings-shell">
    <section className="settings-modal"><div className="settings-layout">
      <aside className="settings-sidebar">
        <header className="settings-sidebar-header"><div><span>iMail 偏好设置</span><h1>设置</h1></div></header>
        <nav className="settings-tabs" aria-label="设置分类">{tabs.map((tab) => { const Icon = tab.icon; return <button type="button" key={tab.id} className={activeTab === tab.id ? 'is-active' : ''} aria-current={activeTab === tab.id ? 'page' : undefined} onClick={() => setActiveTab(tab.id)}><Icon size={18} /><span><strong>{tab.label}</strong><small>{tab.detail}</small></span></button>; })}</nav>
      </aside>
      <main className={`settings-content ${activeTab === 'shortcuts' ? 'has-shortcut-reset' : ''}`}>
        <div className="settings-window-actions">
          {activeTab === 'shortcuts' && <button className="settings-reset-shortcuts" type="button" aria-label="恢复默认快捷键" title="恢复默认快捷键" onClick={() => onBindingsChange({ ...defaultShortcutBindings })}><ArrowCounterClockwise size={20} /></button>}
          <button className="settings-close" type="button" aria-label="关闭设置" title="关闭设置" onClick={onClose}><X size={21} /></button>
        </div>
        {activeTab === 'general' && <GeneralPanel preferences={preferences} onChange={onPreferencesChange} />}
        {activeTab === 'service' && <ServicePanel />}
        {activeTab === 'appearance' && <AppearancePanel preferences={preferences} onChange={onPreferencesChange} />}
        {activeTab === 'accounts' && <AccountSettingsPanel accounts={accounts} onAddAccount={onAddAccount} onReload={onReload} setNotice={setNotice} />}
        {activeTab === 'apple-hme' && <AppleHmeSettingsPanel accounts={accounts} onAddAccount={onAddAccount} setNotice={setNotice} />}
        {activeTab === 'shortcuts' && <ShortcutPanel bindings={bindings} onChange={onBindingsChange} />}
        {activeTab === 'notifications' && <NotificationPanel preferences={preferences} onChange={onPreferencesChange} />}
        {activeTab === 'display' && <DisplayPanel preferences={preferences} onChange={onPreferencesChange} />}
        {activeTab === 'privacy' && <PrivacyPanel accountCount={accounts.length} onReload={onReload} setNotice={setNotice} />}
        {activeTab === 'about' && <AboutPanel />}
      </main>
    </div></section>
  </Overlay>;
}
