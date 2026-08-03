import type { FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowClockwise, Key, PencilSimple, SlidersHorizontal, Trash } from '@phosphor-icons/react';
import type { Account, AccountSyncStatus } from '../../types';
import { credentialGuideFor } from '../../provider-guides';
import { AppInput, AppSelect } from '../../components/form-controls';
import { ProviderIcon, providerLabel } from '../../components/shared';
import { SyncPolicyEditor } from './SyncPolicyEditor';
import { AccountSyncSummary } from './SyncStatusSummary';
import { ProxyFields } from './ProxyFields';

export function AccountSettingsCard({ account, section, syncStatus, workspaceOptions, busy, editing, credentialOpen, removeConfirmOpen, syncEditing, onEdit, onCancelEdit, onUpdateProfile, onRetry, onReconnect, onOpenCredential, onCloseCredential, onUpdateCredential, onOpenRemove, onCloseRemove, onRemove, onOpenSync, onCloseSync, onSaveSync, onQueueSync }: {
  account: Account; section: 'accounts' | 'sync'; syncStatus?: AccountSyncStatus; workspaceOptions: Array<{ value: string; label: string }>; busy: boolean;
  editing: boolean; credentialOpen: boolean; removeConfirmOpen: boolean; syncEditing: boolean;
  onEdit: () => void; onCancelEdit: () => void; onUpdateProfile: (event: FormEvent<HTMLFormElement>) => void; onRetry: () => void; onReconnect: () => void;
  onOpenCredential: () => void; onCloseCredential: () => void; onUpdateCredential: (event: FormEvent<HTMLFormElement>) => void; onOpenRemove: () => void; onCloseRemove: () => void; onRemove: () => void;
  onOpenSync: () => void; onCloseSync: () => void; onSaveSync: (changes: Record<string, unknown>) => Promise<void>; onQueueSync: () => Promise<void>;
}) {
  const connectionText = account.status === 'connected' ? '连接正常' : account.status === 'syncing' ? '正在同步' : account.lastError || '连接异常';
  return <article className={`settings-account-card ${editing ? 'is-editing' : ''} ${syncEditing ? 'is-sync-editing' : ''}`}>
    {section === 'accounts' && editing ? <form className="account-inline-editor" onSubmit={onUpdateProfile}>
      <header className="settings-account-summary"><i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i><span className="account-edit-fields">
        <label className="account-edit-name"><strong>{providerLabel[account.provider]} ·</strong><span className="sr-only">显示名称</span><AppInput name="displayName" defaultValue={account.displayName} maxLength={80} autoFocus required /></label>
        <span className="account-edit-meta"><small title={account.email}>{account.email}</small><span aria-hidden="true">·</span><label className="account-edit-workspace"><span className="sr-only">所属工作空间</span><AppSelect name="group" defaultValue={account.group} options={workspaceOptions} /></label></span>
        <em className={`connection-${account.status}`}>{connectionText}</em>
      </span></header>
      <ProxyFields proxy={account.proxy} compact />
      <footer className="settings-account-actions card-editor-actions"><button type="button" onClick={onCancelEdit}>取消</button><Button appearance="primary" type="submit" disabled={busy}>{busy ? '保存中…' : '保存'}</Button></footer>
    </form> : <>
      <div className="settings-account-main"><header className="settings-account-summary"><i className={`provider-${account.provider}`}><ProviderIcon provider={account.provider} /></i><span><strong>{providerLabel[account.provider]} · {account.displayName}</strong><small>{account.email} · {account.group}</small><em className={`connection-${account.status}`}>{connectionText}</em></span></header>{section === 'sync' && syncStatus && <AccountSyncSummary status={syncStatus} />}</div>
      {section === 'sync' && syncEditing && syncStatus ? <SyncPolicyEditor account={account} status={syncStatus} busy={busy} onSave={onSaveSync} onSync={onQueueSync} onClose={onCloseSync} />
        : credentialOpen ? <form className="credential-renewal" onSubmit={onUpdateCredential}><label><span>{credentialGuideFor(account.provider)?.secretLabel || '新的授权码 / 应用专用密码'}</span><AppInput name="password" type="password" placeholder={credentialGuideFor(account.provider)?.secretPlaceholder || '输入新的专用凭据'} autoFocus required /></label><div className="card-editor-actions"><button type="button" onClick={onCloseCredential}>取消</button><Button appearance="primary" type="submit" disabled={busy}>{busy ? '正在验证…' : '验证并更新'}</Button></div></form>
          : removeConfirmOpen ? <div className="account-remove-confirm"><span><strong>确认移除这个邮箱？</strong><small>{account.email} 的本地邮件缓存也会删除。</small></span><button type="button" onClick={onCloseRemove}>取消</button><button type="button" className="confirm-remove-account" disabled={busy} onClick={onRemove}>{busy ? '正在移除…' : '确认移除'}</button></div>
            : section === 'sync' ? <footer className="settings-account-actions"><button type="button" className="sync-settings-account" disabled={busy || !syncStatus} onClick={onOpenSync}><SlidersHorizontal size={15} />配置同步策略</button><button type="button" disabled={busy || !syncStatus} onClick={onQueueSync}><ArrowClockwise size={15} />立即同步</button></footer>
              : <footer className="settings-account-actions"><button type="button" className="edit-account" disabled={busy} onClick={onEdit}><PencilSimple size={15} />编辑信息</button><button type="button" className="retry-account" disabled={busy} onClick={onRetry}><ArrowClockwise size={15} />{busy ? '正在检查' : '重试连接'}</button>{account.authMethod === 'oauth2' ? <button type="button" className="reconnect-account" disabled={busy} onClick={onReconnect}><Key size={15} />重新授权</button> : <button type="button" className="reconnect-account" disabled={busy} onClick={onOpenCredential}><Key size={15} />更新凭据</button>}<button type="button" className="remove-account" disabled={busy} onClick={onOpenRemove}><Trash size={15} />移除</button></footer>}
    </>}
  </article>;
}
