import { useState } from 'preact/compat';
import { DownloadSimple, LockKey, Trash } from '../../components/icons';
import type { Notice } from '../../app-model';
import { SettingsLinkRow, SettingsPanelHeading } from '../../components/settings-navigation';
import { AuthorizationExport } from './AuthorizationExport';
import { UserDataDeletion } from './UserDataDeletion';

export function PrivacyPanel({ accountCount, onReload, setNotice }: { accountCount: number; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  const [detail, setDetail] = useState<'export' | 'clear' | null>(null);

  if (detail === 'export') return <section className="settings-feature-panel">
    <SettingsPanelHeading title="导出邮箱授权" ancestors={['隐私与数据']} onBack={() => setDetail(null)} />
    <div className="settings-panel-body settings-detail-body"><AuthorizationExport accountCount={accountCount} setNotice={setNotice} /></div>
  </section>;

  if (detail === 'clear') return <section className="settings-feature-panel">
    <SettingsPanelHeading title="清除邮箱数据" ancestors={['隐私与数据']} onBack={() => setDetail(null)} />
    <div className="settings-panel-body settings-detail-body"><UserDataDeletion accountCount={accountCount} onCleared={onReload} setNotice={setNotice} /></div>
  </section>;

  return <section className="settings-feature-panel"><SettingsPanelHeading title="隐私与数据" />
    <div className="settings-panel-body"><div className="privacy-summary"><LockKey size={26} /><div><strong>本地优先</strong><p>邮件缓存和账户配置保存在当前设备，邮箱凭据、OAuth Token 与加密字段不会出现在设置响应中。</p></div></div>
      <div className="settings-link-list settings-link-list-spaced">
        <SettingsLinkRow icon={<DownloadSimple size={20} />} title="导出邮箱授权" detail="创建受密码保护的授权文件，不包含邮件内容。" value={`${accountCount} 个邮箱`} disabled={accountCount === 0} onClick={() => setDetail('export')} />
        <SettingsLinkRow icon={<Trash size={20} />} title="清除邮箱数据" detail="清除当前用户的授权、邮件缓存、草稿和开发者令牌。" danger onClick={() => setDetail('clear')} />
      </div>
      <p className="settings-note">授权导出文件不包含邮件内容；数据清除严格限定为当前登录用户，并要求两阶段确认和当前密码复核。</p></div>
  </section>;
}
