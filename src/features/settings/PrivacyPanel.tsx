import { LockKey } from '@phosphor-icons/react';
import { PanelHeading } from './PanelHeading';

export function PrivacyPanel({ accountCount }: { accountCount: number }) {
  return <section className="settings-feature-panel"><PanelHeading eyebrow="安全边界" title="隐私与数据" description="了解 iMail 如何保存账户、邮件与外部访问凭据。" />
    <div className="settings-panel-body"><div className="privacy-summary"><LockKey size={26} /><div><strong>本地优先</strong><p>邮件缓存和账户配置保存在当前设备，邮箱凭据、OAuth Token 与加密字段不会出现在设置响应中。</p></div></div>
      <dl className="settings-facts"><div><dt>已连接邮箱</dt><dd>{accountCount} 个</dd></div><div><dt>邮件内容</dt><dd>本机缓存</dd></div><div><dt>正文渲染</dt><dd>白名单清洗</dd></div><div><dt>账户管理授权</dt><dd>仅 MCP Full</dd></div></dl>
      <p className="settings-note">删除邮箱账户会同时删除该账户在本机的邮件缓存；执行前会要求二次确认。</p></div>
  </section>;
}
