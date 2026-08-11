import { LockKey } from '@phosphor-icons/react';
import type { Notice } from '../../app-model';
import { PanelHeading } from './PanelHeading';
import { AuthorizationExport } from './AuthorizationExport';
import { UserDataDeletion } from './UserDataDeletion';

export function PrivacyPanel({ accountCount, onReload, setNotice }: { accountCount: number; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  return <section className="settings-feature-panel"><PanelHeading eyebrow="安全边界" title="隐私与数据" description="了解 iMail 如何保存账户、邮件与外部访问凭据。" />
    <div className="settings-panel-body"><div className="privacy-summary"><LockKey size={26} /><div><strong>本地优先</strong><p>邮件缓存和账户配置保存在当前设备，邮箱凭据、OAuth Token 与加密字段不会出现在设置响应中。</p></div></div>
      <dl className="settings-facts"><div><dt>已连接邮箱</dt><dd>{accountCount} 个</dd></div><div><dt>邮件内容</dt><dd>本机缓存</dd></div><div><dt>正文渲染</dt><dd>白名单清洗</dd></div><div><dt>账户管理授权</dt><dd>仅 MCP Full</dd></div></dl>
      <AuthorizationExport accountCount={accountCount} setNotice={setNotice} />
      <UserDataDeletion accountCount={accountCount} onCleared={onReload} setNotice={setNotice} />
      <p className="settings-note">授权导出文件不包含邮件内容；数据清除严格限定为当前登录用户，并要求两阶段确认和当前密码复核。</p></div>
  </section>;
}
