import type { AppPreferences } from '../../app-model';
import { AppCheckbox } from '../../components/form-controls';
import { PanelHeading } from './PanelHeading';

export function NotificationPanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  const options: Array<{ key: keyof AppPreferences['notificationKinds']; title: string; detail: string }> = [
    { key: 'unread', title: '新邮件与未读提醒', detail: '在通知中心显示新到达或仍未读的邮件。' },
    { key: 'snooze', title: '稍后处理到期', detail: '邮件到达设定时间后提醒你继续处理。' },
    { key: 'error', title: '连接与同步异常', detail: '账户授权或后台同步持续失败时显示提醒。' },
  ];
  return <section className="settings-feature-panel"><PanelHeading eyebrow="减少打扰" title="通知" description="选择通知中心保留哪些类型的动态；关键账户错误仍会显示在对应设置中。" />
    <div className="settings-panel-body"><div className="settings-section">{options.map((option) => <label className="settings-row" key={option.key}><span><strong>{option.title}</strong><small>{option.detail}</small></span><AppCheckbox checked={preferences.notificationKinds[option.key]} onChange={(_, data) => onChange({ ...preferences, notificationKinds: { ...preferences.notificationKinds, [option.key]: Boolean(data.checked) } })} /></label>)}</div></div>
  </section>;
}
