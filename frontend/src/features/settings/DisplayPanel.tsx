import type { AppPreferences } from '../../app-model';
import { AppSwitch } from '../../components/form-controls';

export function DisplayPanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  return <section className="settings-section" aria-label="邮件展示">
    <div className="settings-row"><span><strong>渲染邮件</strong><small>开启后安全清洗并保留正文排版；关闭后仅显示纯文本，并阻止远程资源跟踪。</small></span><AppSwitch aria-label="渲染邮件" checked={preferences.defaultMessageView === 'rendered'} onChange={(_, data) => onChange({ ...preferences, defaultMessageView: data.checked ? 'rendered' : 'source' })} /></div>
  </section>;
}
