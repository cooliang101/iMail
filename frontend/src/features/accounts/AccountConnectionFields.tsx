import { ArrowRight, CaretDown, Gear, Key, WarningCircle } from '../../components/icons';
import type { CredentialGuide } from '../../config/provider-guides';
import type { ProviderId } from '../../types';
import { providerLabel } from '../../components/shared';
import { AppInput, AppSelect, type AppSelectOption } from '../../components/form-controls';
import { ProxyFields } from './ProxyFields';

export function AccountConnectionFields({ provider, usesOAuth, oauthConfigured, credentialGuide, workspaceOptions, busy, advanced, onAdvancedChange, onManualModeChange, onCancelOAuth }: {
  provider: ProviderId; usesOAuth: boolean; oauthConfigured?: boolean; credentialGuide?: CredentialGuide; workspaceOptions: AppSelectOption[]; busy: boolean; advanced: boolean;
  onAdvancedChange: (advanced: boolean) => void; onManualModeChange: (manual: boolean) => void; onCancelOAuth: () => void;
}) {
  return <>
    {usesOAuth ? <><div className="oauth-panel">
      <div className={`oauth-status ${oauthConfigured ? 'ready' : 'setup'}`}><Key size={21} weight="duotone" /><span><strong>使用 {providerLabel[provider]} 登录</strong><small>{oauthConfigured ? '你将在服务商官方页面完成登录，iMail 不会接触你的网页登录密码。' : '快捷登录暂时不可用，你可以稍后重试或选择其他连接方式。'}</small></span></div>
      <div className="form-grid oauth-profile"><label><span>显示名称（可选）</span><AppInput name="displayName" placeholder="默认使用账户名称" /></label><label><span>加入工作空间</span><AppSelect name="group" defaultValue="工作" options={workspaceOptions} /></label></div>
      {provider === 'yahoo' && <div className="oauth-review"><WarningCircle size={17} /><span>若快捷登录暂不可用，可改用 Yahoo 应用专用密码。</span></div>}
      {busy && <div className="oauth-waiting"><span><strong>正在等待 {providerLabel[provider]} 授权</strong><small>如果服务商页面显示配置错误，请关闭授权窗口或结束等待，修正后可以直接重试。</small></span><button type="button" onClick={onCancelOAuth}>结束等待</button></div>}
    </div>{credentialGuide && <button type="button" className="manual-switch" onClick={() => onManualModeChange(true)}>改用应用专用密码</button>}</> : <>
      {credentialGuide && <section className="credential-guide"><div className="credential-guide-heading"><Key size={21} weight="duotone" /><span><strong>{credentialGuide.title}</strong><small>{credentialGuide.description}</small></span><a href={credentialGuide.helpUrl} target="_blank" rel="noreferrer">{credentialGuide.actionLabel}<ArrowRight size={14} /></a></div><ol>{credentialGuide.steps.map((step, index) => <li key={step}><b>{index + 1}</b><span>{step}</span></li>)}</ol></section>}
      <div className="form-grid"><label><span>邮箱地址</span><AppInput name="email" type="email" placeholder="name@example.com" required /></label><label><span>显示名称</span><AppInput name="displayName" placeholder="例如：工作邮箱" required /></label><label><span>工作空间</span><AppSelect name="group" defaultValue="工作" options={workspaceOptions} /></label><label><span>{credentialGuide?.secretLabel || '应用专用密码 / 授权码'}</span><AppInput name="password" type="password" placeholder={credentialGuide?.secretPlaceholder || '不会以明文保存'} required /></label></div>
      {provider !== 'custom' && !credentialGuide && <div className="provider-tip"><Key size={19} /><span><strong>{providerLabel[provider]} 安全提示</strong><small>请使用服务商提供的专用凭据，不要填写网页登录密码。</small></span></div>}
      {['outlook', 'gmail', 'yahoo', 'hotmail'].includes(provider) && oauthConfigured && <button type="button" className="manual-switch" onClick={() => onManualModeChange(false)}>返回 {providerLabel[provider]} 快捷登录</button>}
    </>}
    {provider === 'custom' && <div className="advanced-settings"><button type="button" onClick={() => onAdvancedChange(!advanced)}><Gear size={17} />IMAP / SMTP 设置<CaretDown size={15} /></button>{(advanced || provider === 'custom') && <div className="form-grid"><label><span>IMAP 主机</span><AppInput name="imapHost" placeholder="imap.example.com" required /></label><label><span>IMAP 端口</span><AppInput name="imapPort" type="number" defaultValue="993" required /></label><label><span>SMTP 主机</span><AppInput name="smtpHost" placeholder="smtp.example.com" required /></label><label><span>SMTP 端口</span><AppInput name="smtpPort" type="number" defaultValue="465" required /></label></div>}</div>}
    <div className="advanced-settings"><button type="button" onClick={() => onAdvancedChange(!advanced)}><Gear size={17} />网络代理（可选）<CaretDown size={15} /></button>{advanced && <ProxyFields />}</div>
  </>;
}
