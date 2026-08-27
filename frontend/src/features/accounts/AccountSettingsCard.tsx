import type { FormEvent } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { ArrowClockwise, Globe, Key, PencilSimple, Trash } from '../../components/icons';
import { SettingsLinkRow } from '../../components/settings-navigation';
import type { Account } from '../../types';
import { credentialGuideFor } from '../../config/provider-guides';
import { AppInput, AppSelect } from '../../components/form-controls';
import { ProviderIcon, providerLabel } from '../../components/shared';
import { ProxyFields, type ProxyPreset } from './ProxyFields';

export function AccountSettingsCard({ account, proxyPresets, workspaceOptions, busy, editing, proxyEditing, credentialOpen, removeConfirmOpen, onEdit, onCancelEdit, onUpdateProfile, onOpenProxy, onCloseProxy, onUpdateProxy, onRetry, onReconnect, onOpenCredential, onCloseCredential, onUpdateCredential, onOpenRemove, onCloseRemove, onRemove }: {
  account: Account; proxyPresets: ProxyPreset[]; workspaceOptions: Array<{ value: string; label: string }>; busy: boolean;
  editing: boolean; proxyEditing: boolean; credentialOpen: boolean; removeConfirmOpen: boolean;
  onEdit: () => void; onCancelEdit: () => void; onUpdateProfile: (event: FormEvent<HTMLFormElement>) => void; onRetry: () => void; onReconnect: () => void;
  onOpenProxy: () => void; onCloseProxy: () => void; onUpdateProxy: (event: FormEvent<HTMLFormElement>) => void;
  onOpenCredential: () => void; onCloseCredential: () => void; onUpdateCredential: (event: FormEvent<HTMLFormElement>) => void; onOpenRemove: () => void; onCloseRemove: () => void; onRemove: () => void;
}) {
  const connectionText = account.status === 'connected' ? '连接正常' : account.status === 'syncing' ? '正在同步' : account.lastError || '连接异常';
  const proxyText = account.proxy ? `${account.proxy.protocol.toUpperCase()} · ${account.proxy.host}:${account.proxy.port}` : '直连（未使用代理）';
  return <article className="account-detail-view">
    <div className="settings-account-main"><header className="settings-account-summary"><i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i><span><strong>{providerLabel[account.provider]} · {account.displayName}</strong><small>{account.email} · {account.group}</small><span className="account-status-line"><em className={`connection-${account.status}`}>{connectionText}</em></span></span></header></div>
    <div className="account-detail-sections">
      <section className="account-detail-section" aria-labelledby={`account-settings-${account.id}`}>
        <h3 id={`account-settings-${account.id}`}>账户设置</h3>
        <div className="settings-link-list">
          <SettingsLinkRow icon={<PencilSimple />} title="编辑信息" detail="修改显示名称和所属工作空间" expanded={editing} onClick={editing ? onCancelEdit : onEdit} disabled={busy} />
          {editing && <form className="account-inline-expansion account-inline-editor account-profile-editor" onSubmit={onUpdateProfile}>
            <div className="account-detail-form-grid">
              <label><span>显示名称</span><AppInput name="displayName" defaultValue={account.displayName} maxLength={80} autoFocus required /></label>
              <label><span>所属工作空间</span><AppSelect name="group" defaultValue={account.group} options={workspaceOptions} /></label>
            </div>
            <footer className="settings-account-actions card-editor-actions"><button type="button" onClick={onCancelEdit}>取消</button><AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '保存中…' : '保存'}</AppButton></footer>
          </form>}
          <SettingsLinkRow icon={<Globe />} title="网络代理" detail={proxyText} value={account.proxy ? '已启用' : '直连'} expanded={proxyEditing} onClick={proxyEditing ? onCloseProxy : onOpenProxy} disabled={busy} />
          {proxyEditing && <form className="account-inline-expansion account-inline-editor account-proxy-editor" onSubmit={onUpdateProxy}>
            <ProxyFields proxy={account.proxy} presets={proxyPresets} compact />
            <footer className="settings-account-actions card-editor-actions"><button type="button" onClick={onCloseProxy}>取消</button><AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '正在验证…' : '验证并保存代理'}</AppButton></footer>
          </form>}
          <SettingsLinkRow icon={<ArrowClockwise />} title="检查连接" detail="使用当前授权立即验证邮箱连接" value={busy ? '检查中…' : connectionText} disclosure={false} onClick={onRetry} disabled={busy} />
          {account.authMethod === 'oauth2'
            ? <SettingsLinkRow icon={<Key />} title="重新授权" detail="重新完成 OAuth 授权并刷新登录凭据" value="OAuth" disclosure={false} onClick={onReconnect} disabled={busy} />
            : <><SettingsLinkRow icon={<Key />} title="更新凭据" detail="更换授权码或应用专用密码" value="专用凭据" expanded={credentialOpen} onClick={credentialOpen ? onCloseCredential : onOpenCredential} disabled={busy} />
              {credentialOpen && <form className="account-inline-expansion credential-renewal" onSubmit={onUpdateCredential}>
                <label><span>{credentialGuideFor(account.provider)?.secretLabel || '新的授权码 / 应用专用密码'}</span><AppInput name="password" type="password" placeholder={credentialGuideFor(account.provider)?.secretPlaceholder || '输入新的专用凭据'} autoFocus required /></label>
                <div className="card-editor-actions"><button type="button" onClick={onCloseCredential}>取消</button><AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '正在验证…' : '验证并更新'}</AppButton></div>
              </form>}</>}
        </div>
      </section>
      <section className="account-detail-section is-danger" aria-labelledby={`account-danger-${account.id}`}>
        <h3 id={`account-danger-${account.id}`}>危险操作</h3>
        <div className="settings-link-list">
          <SettingsLinkRow icon={<Trash />} title="移除邮箱" detail="删除账户配置及其本地邮件缓存" danger expanded={removeConfirmOpen} onClick={removeConfirmOpen ? onCloseRemove : onOpenRemove} disabled={busy} />
          {removeConfirmOpen && <div className="account-inline-expansion account-remove-confirm"><span><strong>确认移除这个邮箱？</strong><small>{account.email} 的账户配置与本地邮件缓存都会删除，此操作无法撤销。</small></span><div className="account-remove-actions"><button type="button" onClick={onCloseRemove}>取消</button><button type="button" className="confirm-remove-account" disabled={busy} onClick={onRemove}>{busy ? '正在移除…' : '确认移除'}</button></div></div>}
        </div>
      </section>
    </div>
  </article>;
}
