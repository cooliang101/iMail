import type { FormEvent } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { ArrowClockwise, Globe, Key, PencilSimple, Trash } from '../../components/icons';
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
  return <article className={`settings-account-card ${editing || proxyEditing ? 'is-editing' : ''}`}>
    {editing ? <form className="account-inline-editor" onSubmit={onUpdateProfile}>
      <header className="settings-account-summary"><i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i><span className="account-edit-fields">
        <label className="account-edit-name"><strong>{providerLabel[account.provider]} ·</strong><span className="sr-only">显示名称</span><AppInput name="displayName" defaultValue={account.displayName} maxLength={80} autoFocus required /></label>
        <span className="account-edit-meta"><small title={account.email}>{account.email}</small><span aria-hidden="true">·</span><label className="account-edit-workspace"><span className="sr-only">所属工作空间</span><AppSelect name="group" defaultValue={account.group} options={workspaceOptions} /></label></span>
        <em className={`connection-${account.status}`}>{connectionText}</em>
      </span></header>
      <footer className="settings-account-actions card-editor-actions"><button type="button" onClick={onCancelEdit}>取消</button><AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '保存中…' : '保存'}</AppButton></footer>
    </form> : proxyEditing ? <form className="account-inline-editor account-proxy-editor" onSubmit={onUpdateProxy}>
      <header className="account-proxy-editor-heading"><Globe size={24} weight="duotone" /><div><strong>{account.displayName} 的网络代理</strong><small>{account.email} · 当前为{proxyText}</small></div></header>
      <ProxyFields proxy={account.proxy} presets={proxyPresets} compact />
      <footer className="settings-account-actions card-editor-actions"><button type="button" onClick={onCloseProxy}>取消</button><AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '正在验证…' : '验证并保存代理'}</AppButton></footer>
    </form> : <>
      <div className="settings-account-main"><header className="settings-account-summary"><i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i><span><strong>{providerLabel[account.provider]} · {account.displayName}</strong><small>{account.email} · {account.group}</small><span className="account-status-line"><em className={`connection-${account.status}`}>{connectionText}</em><em className={`account-proxy-status ${account.proxy ? 'is-enabled' : ''}`}><Globe size={12} />{proxyText}</em></span></span></header></div>
      {credentialOpen ? <form className="credential-renewal" onSubmit={onUpdateCredential}><label><span>{credentialGuideFor(account.provider)?.secretLabel || '新的授权码 / 应用专用密码'}</span><AppInput name="password" type="password" placeholder={credentialGuideFor(account.provider)?.secretPlaceholder || '输入新的专用凭据'} autoFocus required /></label><div className="card-editor-actions"><button type="button" onClick={onCloseCredential}>取消</button><AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '正在验证…' : '验证并更新'}</AppButton></div></form>
          : removeConfirmOpen ? <div className="account-remove-confirm"><span><strong>确认移除这个邮箱？</strong><small>{account.email} 的本地邮件缓存也会删除。</small></span><button type="button" onClick={onCloseRemove}>取消</button><button type="button" className="confirm-remove-account" disabled={busy} onClick={onRemove}>{busy ? '正在移除…' : '确认移除'}</button></div>
            : <footer className="settings-account-actions"><button type="button" className="edit-account" disabled={busy} onClick={onEdit}><PencilSimple size={15} />编辑信息</button><button type="button" className="proxy-settings-account" disabled={busy} onClick={onOpenProxy}><Globe size={15} />代理设置</button><button type="button" className="retry-account" disabled={busy} onClick={onRetry}><ArrowClockwise size={15} />{busy ? '正在检查' : '重试连接'}</button>{account.authMethod === 'oauth2' ? <button type="button" className="reconnect-account" disabled={busy} onClick={onReconnect}><Key size={15} />重新授权</button> : <button type="button" className="reconnect-account" disabled={busy} onClick={onOpenCredential}><Key size={15} />更新凭据</button>}<button type="button" className="remove-account" disabled={busy} onClick={onOpenRemove}><Trash size={15} />移除</button></footer>}
    </>}
  </article>;
}
