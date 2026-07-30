import { useEffect, useState, type KeyboardEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowCounterClockwise, Bell, Check, Database, Envelope, Eye, Gear, Keyboard, LockKey, SlidersHorizontal, X } from '@phosphor-icons/react';
import type { Account } from '../../types';
import type { AppPreferences, Notice, ShortcutActionId, ShortcutBindings } from '../../app-model';
import { Overlay } from '../../components/shared';
import { AppCheckbox, AppSelect } from '../../components/form-controls';
import { AccountSettingsPanel } from '../accounts';
import { defaultShortcutBindings, shortcutConflict, shortcutDefinitions, shortcutFromEvent, shortcutLabel } from '../shortcuts';

export type SettingsTab = 'general' | 'accounts' | 'sync' | 'shortcuts' | 'notifications' | 'display' | 'privacy';

const tabs: Array<{ id: SettingsTab; label: string; detail: string; icon: typeof Gear }> = [
  { id: 'general', label: '通用', detail: '启动与阅读行为', icon: Gear },
  { id: 'accounts', label: '邮箱账号', detail: '连接、授权与工作空间', icon: Envelope },
  { id: 'sync', label: '同步', detail: '频率、范围与状态', icon: SlidersHorizontal },
  { id: 'shortcuts', label: '快捷键', detail: '键盘操作与绑定', icon: Keyboard },
  { id: 'notifications', label: '通知', detail: '选择需要关注的动态', icon: Bell },
  { id: 'display', label: '邮件展示', detail: '正文默认查看方式', icon: Eye },
  { id: 'privacy', label: '隐私与数据', detail: '本地优先与安全边界', icon: LockKey },
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
    <section className="settings-modal">
      <header className="settings-titlebar"><div><span>iMail 偏好设置</span><h1>设置</h1></div><button type="button" aria-label="关闭设置" onClick={onClose}><X size={21} /></button></header>
      <div className="settings-layout">
        <nav className="settings-tabs" aria-label="设置分类">{tabs.map((tab) => { const Icon = tab.icon; return <button type="button" key={tab.id} className={activeTab === tab.id ? 'is-active' : ''} aria-current={activeTab === tab.id ? 'page' : undefined} onClick={() => setActiveTab(tab.id)}><Icon size={18} /><span><strong>{tab.label}</strong><small>{tab.detail}</small></span></button>; })}</nav>
        <main className="settings-content">
          {activeTab === 'general' && <GeneralPanel preferences={preferences} onChange={onPreferencesChange} />}
          {(activeTab === 'accounts' || activeTab === 'sync') && <AccountSettingsPanel accounts={accounts} section={activeTab} onAddAccount={onAddAccount} onReload={onReload} setNotice={setNotice} />}
          {activeTab === 'shortcuts' && <ShortcutPanel bindings={bindings} onChange={onBindingsChange} />}
          {activeTab === 'notifications' && <NotificationPanel preferences={preferences} onChange={onPreferencesChange} />}
          {activeTab === 'display' && <DisplayPanel preferences={preferences} onChange={onPreferencesChange} />}
          {activeTab === 'privacy' && <PrivacyPanel accountCount={accounts.length} />}
        </main>
      </div>
    </section>
  </Overlay>;
}

function PanelHeading({ eyebrow, title, description }: { eyebrow: string; title: string; description: string }) {
  return <header className="settings-panel-heading"><div><span>{eyebrow}</span><h2>{title}</h2><p>{description}</p></div><small>自动同步到服务端，并在本机缓存</small></header>;
}

function GeneralPanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  return <section className="settings-feature-panel"><PanelHeading eyebrow="应用行为" title="通用" description="控制 iMail 启动后的默认位置和邮件状态变化。" />
    <div className="settings-section"><label className="settings-row"><span><strong>启动页面</strong><small>下次打开 iMail 时首先显示的邮箱范围。</small></span><AppSelect value={preferences.startupView} onValueChange={(startupView) => onChange({ ...preferences, startupView: startupView as AppPreferences['startupView'] })} options={[{ value: 'inbox', label: '统一收件箱' }, { value: 'starred', label: '已加星标' }]} /></label>
      <label className="settings-row"><span><strong>打开邮件时标记为已读</strong><small>关闭后，阅读邮件不会自动改变未读状态。</small></span><AppCheckbox checked={preferences.markReadOnOpen} onChange={(_, data) => onChange({ ...preferences, markReadOnOpen: Boolean(data.checked) })} /></label>
    </div>
  </section>;
}

function DisplayPanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  return <section className="settings-feature-panel"><PanelHeading eyebrow="阅读体验" title="邮件展示" description="选择每封邮件正文首次打开时的查看方式，仍可在邮件内随时切换。" />
    <div className="display-choice-grid" role="radiogroup" aria-label="默认邮件正文视图">
      <button type="button" role="radio" aria-checked={preferences.defaultMessageView === 'source'} className={preferences.defaultMessageView === 'source' ? 'is-selected' : ''} onClick={() => onChange({ ...preferences, defaultMessageView: 'source' })}><Database size={22} /><span><strong>原始内容</strong><small>直接显示服务器返回的 HTML 源码或纯文本，便于检查邮件结构。</small></span><i>{preferences.defaultMessageView === 'source' && <Check size={14} />}</i></button>
      <button type="button" role="radio" aria-checked={preferences.defaultMessageView === 'rendered'} className={preferences.defaultMessageView === 'rendered' ? 'is-selected' : ''} onClick={() => onChange({ ...preferences, defaultMessageView: 'rendered' })}><Eye size={22} /><span><strong>渲染邮件</strong><small>在受限 iframe 中按邮件设计排版，脚本、对象和表单均被禁用。</small></span><i>{preferences.defaultMessageView === 'rendered' && <Check size={14} />}</i></button>
    </div>
  </section>;
}

function NotificationPanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  const options: Array<{ key: keyof AppPreferences['notificationKinds']; title: string; detail: string }> = [
    { key: 'unread', title: '新邮件与未读提醒', detail: '在通知中心显示新到达或仍未读的邮件。' },
    { key: 'snooze', title: '稍后处理到期', detail: '邮件到达设定时间后提醒你继续处理。' },
    { key: 'error', title: '连接与同步异常', detail: '账户授权或后台同步持续失败时显示提醒。' },
  ];
  return <section className="settings-feature-panel"><PanelHeading eyebrow="减少打扰" title="通知" description="选择通知中心保留哪些类型的动态；关键账户错误仍会显示在对应设置中。" />
    <div className="settings-section">{options.map((option) => <label className="settings-row" key={option.key}><span><strong>{option.title}</strong><small>{option.detail}</small></span><AppCheckbox checked={preferences.notificationKinds[option.key]} onChange={(_, data) => onChange({ ...preferences, notificationKinds: { ...preferences.notificationKinds, [option.key]: Boolean(data.checked) } })} /></label>)}</div>
  </section>;
}

function PrivacyPanel({ accountCount }: { accountCount: number }) {
  return <section className="settings-feature-panel"><PanelHeading eyebrow="安全边界" title="隐私与数据" description="了解 iMail 如何保存账户、邮件与外部访问凭据。" />
    <div className="privacy-summary"><LockKey size={26} /><div><strong>本地优先</strong><p>邮件缓存和账户配置保存在当前设备，邮箱凭据、OAuth Token 与加密字段不会出现在设置响应中。</p></div></div>
    <dl className="settings-facts"><div><dt>已连接邮箱</dt><dd>{accountCount} 个</dd></div><div><dt>邮件内容</dt><dd>本机缓存</dd></div><div><dt>渲染隔离</dt><dd>受限 iframe</dd></div><div><dt>账户管理授权</dt><dd>仅 MCP Full</dd></div></dl>
    <p className="settings-note">删除邮箱账户会同时删除该账户在本机的邮件缓存；执行前会要求二次确认。</p>
  </section>;
}

function ShortcutPanel({ bindings, onChange }: { bindings: ShortcutBindings; onChange: (bindings: ShortcutBindings) => void }) {
  const [draft, setDraft] = useState(bindings);
  const [recording, setRecording] = useState<ShortcutActionId | null>(null);
  const [error, setError] = useState('');
  useEffect(() => setDraft(bindings), [bindings]);
  function capture(event: KeyboardEvent<HTMLButtonElement>, actionId: ShortcutActionId) {
    if (recording !== actionId) return;
    event.preventDefault(); event.stopPropagation();
    if (event.key === 'Escape') { setRecording(null); setError(''); return; }
    if (event.key === 'Backspace' || event.key === 'Delete') { setDraft((current) => ({ ...current, [actionId]: '' })); setRecording(null); setError(''); return; }
    const candidate = shortcutFromEvent(event.nativeEvent);
    if (!candidate) return;
    const conflict = shortcutConflict(draft, actionId, candidate);
    if (conflict) { setError(`“${shortcutLabel(candidate)}”已用于“${conflict.label}”`); return; }
    setDraft((current) => ({ ...current, [actionId]: candidate })); setRecording(null); setError('');
  }
  return <section className="settings-feature-panel"><header className="settings-panel-heading"><div><span>键盘效率</span><h2>快捷键</h2><p>点击绑定后按下新组合键；Backspace 清除，Esc 取消录制。</p></div><button type="button" onClick={() => { setDraft({ ...defaultShortcutBindings }); setError(''); }}><ArrowCounterClockwise size={15} />恢复默认</button></header>
    <div className="shortcut-groups">{(['global', 'mail'] as const).map((scope) => <section key={scope}><h3>{scope === 'global' ? '全局操作' : '邮件操作'}</h3><div className="shortcut-list">{shortcutDefinitions.filter((item) => item.scope === scope).map((item) => <div className="shortcut-row" key={item.id}><i><Keyboard size={18} /></i><span><strong>{item.label}</strong><small>{item.description}</small></span><button type="button" className={recording === item.id ? 'is-recording' : ''} onClick={() => { setRecording(item.id); setError(''); }} onKeyDown={(event) => capture(event, item.id)}>{recording === item.id ? '请按键…' : <kbd>{shortcutLabel(draft[item.id])}</kbd>}</button></div>)}</div></section>)}</div>
    {error && <div className="shortcut-error">{error}</div>}<footer className="settings-sticky-actions"><Button appearance="primary" onClick={() => onChange(draft)}>保存快捷键</Button></footer>
  </section>;
}
