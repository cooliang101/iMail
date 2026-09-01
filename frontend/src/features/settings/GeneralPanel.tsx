import { useState } from 'preact/compat';
import type { AppPreferences } from '../../app-model';
import { AppSelect, AppSwitch } from '../../components/form-controls';
import { SettingsPanelHeading } from '../../components/settings-navigation';
import { useI18n } from '../i18n';
import { ServiceAddressEditor, ServicePanel } from '../service';
import { DisplayPanel } from './DisplayPanel';

export function GeneralPanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  const { t } = useI18n();
  const [remoteEditorOpen, setRemoteEditorOpen] = useState(false);
  if (remoteEditorOpen) return <section className="settings-feature-panel">
    <SettingsPanelHeading title={t('远程服务')} ancestors={[t('通用')]} onBack={() => setRemoteEditorOpen(false)} />
    <div className="settings-panel-body app-scrollbar service-settings-panel settings-detail-body">
      <ServiceAddressEditor onCancel={() => setRemoteEditorOpen(false)} onSaved={() => setRemoteEditorOpen(false)} />
    </div>
  </section>;
  return <section className="settings-feature-panel"><SettingsPanelHeading title={t('通用')} />
    <div className="settings-panel-body app-scrollbar general-settings-body"><div className="settings-section">
      <div className="settings-row"><span><strong>{t('界面语言')}</strong><small>{t('选择 iMail 界面使用的语言。')}</small></span><AppSelect className="settings-startup-select" value={preferences.language} onValueChange={(language) => onChange({ ...preferences, language: language as AppPreferences['language'] })} options={[{ value: 'zh-CN', label: t('简体中文') }, { value: 'en-US', label: 'English' }]} /></div>
      <div className="settings-row"><span><strong>{t('启动页面')}</strong><small>{t('下次打开 iMail 时首先显示的邮箱范围。')}</small></span><AppSelect className="settings-startup-select" listboxClassName="settings-startup-listbox" value={preferences.startupView} onValueChange={(startupView) => onChange({ ...preferences, startupView: startupView as AppPreferences['startupView'] })} options={[{ value: 'inbox', label: t('统一收件箱') }, { value: 'starred', label: t('已加星标') }]} /></div>
      <div className="settings-row"><span><strong>{t('打开邮件时标记为已读')}</strong><small>{t('关闭后，阅读邮件不会自动改变未读状态。')}</small></span><AppSwitch aria-label={t('打开邮件时标记为已读')} checked={preferences.markReadOnOpen} onChange={(_, data) => onChange({ ...preferences, markReadOnOpen: Boolean(data.checked) })} /></div></div>
      <DisplayPanel preferences={preferences} onChange={onChange} />
      <ServicePanel onEditRemote={() => setRemoteEditorOpen(true)} />
    </div>
  </section>;
}
