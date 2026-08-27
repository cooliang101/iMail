import type { AppPreferences } from '../../app-model';
import { AppCheckbox, AppSelect } from '../../components/form-controls';
import { SettingsPanelHeading } from '../../components/settings-navigation';

export function GeneralPanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  return <section className="settings-feature-panel"><SettingsPanelHeading title="通用" />
    <div className="settings-panel-body"><div className="settings-section"><label className="settings-row"><span><strong>启动页面</strong><small>下次打开 iMail 时首先显示的邮箱范围。</small></span><AppSelect className="settings-startup-select" value={preferences.startupView} onValueChange={(startupView) => onChange({ ...preferences, startupView: startupView as AppPreferences['startupView'] })} options={[{ value: 'inbox', label: '统一收件箱' }, { value: 'starred', label: '已加星标' }]} /></label>
      <label className="settings-row"><span><strong>打开邮件时标记为已读</strong><small>关闭后，阅读邮件不会自动改变未读状态。</small></span><AppCheckbox checked={preferences.markReadOnOpen} onChange={(_, data) => onChange({ ...preferences, markReadOnOpen: Boolean(data.checked) })} /></label></div>
    </div>
  </section>;
}
