import { useState } from 'preact/compat';
import '../../styles/dialogs.css';
import '../../styles/settings.css';
import '../../styles/settings-responsive.css';
import { ArrowCounterClockwise, Bell, Cloud, Envelope, Gear, Globe, Info, Keyboard, LockKey, Palette, X } from '../../components/icons';
import type { Account, ProviderId } from '../../types';
import type { AppPreferences, Notice, ShortcutBindings } from '../../app-model';
import { Overlay } from '../../components/shared';
import { AccountSettingsPanel } from '../accounts';
import { defaultShortcutBindings } from '../shortcuts';
import { AppearancePanel } from './AppearancePanel';
import { GeneralPanel } from './GeneralPanel';
import { NotificationPanel } from './NotificationPanel';
import { PrivacyPanel } from './PrivacyPanel';
import { ShortcutPanel } from './ShortcutPanel';
import { AppleHmeSettingsPanel } from './AppleHmeSettingsPanel';
import { AboutPanel } from './AboutPanel';
import { TranslationSettingsPanel } from '../translation';
import { useI18n } from '../i18n';
import { CompositionSettingsPanel } from '../compose/CompositionSettingsPanel';
import { RulesSettingsPanel } from '../rules/RulesSettingsPanel';

export type SettingsTab = 'general' | 'appearance' | 'accounts' | 'apple-hme' | 'translation' | 'shortcuts' | 'notifications' | 'privacy' | 'about' | 'composition' | 'rules';

const tabs: Array<{ id: SettingsTab; label: string; detail: string; icon: typeof Gear }> = [
  { id: 'general', label: '通用', detail: '启动与阅读行为', icon: Gear },
  { id: 'appearance', label: '主题', detail: '界面风格与色彩', icon: Palette },
  { id: 'accounts', label: '邮箱管理', detail: '授权、代理与工作空间', icon: Envelope },
  { id: 'composition', label: '写信', detail: '账户签名与模板', icon: Envelope },
  { id: 'rules', label: '邮件规则', detail: '自动整理与执行记录', icon: Gear },
  { id: 'apple-hme', label: '隐私邮箱', detail: 'iCloud Hide My Email', icon: Cloud },
  { id: 'translation', label: '翻译服务', detail: '翻译服务与语言', icon: Globe },
  { id: 'shortcuts', label: '快捷键', detail: '键盘操作与绑定', icon: Keyboard },
  { id: 'notifications', label: '通知', detail: '选择需要关注的动态', icon: Bell },
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
  onAddAccount: (provider?: ProviderId, returnTo?: SettingsTab) => void;
  onReload: () => Promise<void>;
  setNotice: (notice: Notice) => void;
  onClose: () => void;
}) {
  const [activeTab, setActiveTab] = useState(initialTab);
  const { t } = useI18n();
  return <Overlay onClose={onClose} wide dialogClassName="settings-shell">
    <section className="settings-modal"><div className="settings-layout">
      <aside className="settings-sidebar">
        <header className="settings-sidebar-header"><div><span>{t('iMail 偏好设置')}</span><h1>{t('设置')}</h1></div></header>
        <nav className="settings-tabs app-scrollbar" aria-label={t('设置分类')}>{tabs.map((tab) => { const Icon = tab.icon; return <button type="button" key={tab.id} className={activeTab === tab.id ? 'is-active' : ''} aria-current={activeTab === tab.id ? 'page' : undefined} onClick={() => setActiveTab(tab.id)}><Icon size={18} /><span><strong>{t(tab.label)}</strong><small>{t(tab.detail)}</small></span></button>; })}</nav>
      </aside>
      <main className={`settings-content ${activeTab === 'shortcuts' ? 'has-shortcut-reset' : ''}`}>
        <div className="settings-window-actions">
          {activeTab === 'shortcuts' && <button className="settings-reset-shortcuts" type="button" aria-label={t('恢复默认快捷键')} title={t('恢复默认快捷键')} onClick={() => onBindingsChange({ ...defaultShortcutBindings })}><ArrowCounterClockwise size={20} /></button>}
          <button className="settings-close" type="button" aria-label={t('关闭设置')} title={t('关闭设置')} onClick={onClose}><X size={21} /></button>
        </div>
        {activeTab === 'general' && <GeneralPanel preferences={preferences} onChange={onPreferencesChange} />}
        {activeTab === 'composition' && <CompositionSettingsPanel accounts={accounts} preferences={preferences} onChange={onPreferencesChange} />}
        {activeTab === 'rules' && <RulesSettingsPanel accounts={accounts} onReload={onReload} />}
        {activeTab === 'appearance' && <AppearancePanel preferences={preferences} onChange={onPreferencesChange} />}
        {activeTab === 'accounts' && <AccountSettingsPanel accounts={accounts} onAddAccount={() => onAddAccount(undefined, 'accounts')} onReload={onReload} setNotice={setNotice} />}
        {activeTab === 'apple-hme' && <AppleHmeSettingsPanel accounts={accounts} onAddAccount={() => onAddAccount('icloud', 'apple-hme')} setNotice={setNotice} />}
        {activeTab === 'translation' && <TranslationSettingsPanel setNotice={setNotice} />}
        {activeTab === 'shortcuts' && <ShortcutPanel bindings={bindings} onChange={onBindingsChange} />}
        {activeTab === 'notifications' && <NotificationPanel preferences={preferences} onChange={onPreferencesChange} />}
        {activeTab === 'privacy' && <PrivacyPanel accountCount={accounts.length} onReload={onReload} setNotice={setNotice} />}
        {activeTab === 'about' && <AboutPanel />}
      </main>
    </div></section>
  </Overlay>;
}
