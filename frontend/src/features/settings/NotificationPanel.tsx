import { useState } from 'preact/compat';
import type { AppPreferences } from '../../app-model';
import { AppSwitch } from '../../components/form-controls';
import { usePlatform } from '../../platform/runtime';
import { SettingsPanelHeading } from '../../components/settings-navigation';

export function NotificationPanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  const platform = usePlatform();
  const [testResult, setTestResult] = useState('');
  const options: Array<{ key: keyof AppPreferences['notificationKinds']; title: string; detail: string }> = [
    { key: 'unread', title: '新邮件与未读提醒', detail: '新邮件到达时发送系统通知，并在通知中心保留未读邮件。' },
    { key: 'snooze', title: '稍后处理到期', detail: '邮件到达设定时间后提醒你继续处理。' },
    { key: 'error', title: '连接与同步异常', detail: '账户授权或后台同步持续失败时显示提醒。' },
  ];
  return <section className="settings-feature-panel"><SettingsPanelHeading title="通知" />
    <div className="settings-panel-body"><div className="settings-section">{options.map((option) => <div className="settings-row" key={option.key}><span><strong>{option.title}</strong><small>{option.detail}</small></span><AppSwitch aria-label={option.title} checked={preferences.notificationKinds[option.key]} onChange={(_, data) => onChange({ ...preferences, notificationKinds: { ...preferences.notificationKinds, [option.key]: Boolean(data.checked) } })} /></div>)}</div>
      <div className="settings-sticky-actions"><button type="button" className="settings-primary-action" onClick={() => {
        setTestResult('正在发送…');
        void platform.notify({ title: 'iMail 通知测试', body: '系统通知已配置成功。' })
          .then(() => setTestResult('测试通知已发送'))
          .catch((error) => setTestResult(error instanceof Error ? error.message : '测试通知发送失败'));
      }}>发送测试通知</button></div>
      {testResult && <p className="settings-notification-test-result">{testResult}</p>}
    </div>
  </section>;
}
