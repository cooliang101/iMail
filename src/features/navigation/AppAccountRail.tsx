import type { CSSProperties, MouseEvent } from 'react';
import { Gear, Plus, Tray } from '@phosphor-icons/react';
import type { Account } from '../../types';
import { BrandLogo } from '../../components/brand-logo';
import { ProviderIcon, providerLabel } from '../../components/provider-icons';

export function AppAccountRail({ accounts, accountFilter, onSelect, onAdd, onSettings, onContextMenu }: {
  accounts: Account[]; accountFilter: string; onSelect: (accountId?: string) => void; onAdd: () => void; onSettings: () => void;
  onContextMenu: (event: MouseEvent, accountId?: string) => void;
}) {
  return <aside className="account-rail" aria-label="邮箱账户">
    <button className="brand-mark" aria-label="iMail"><BrandLogo /></button>
    <div className="rail-accounts">
      <button title="聚合所有邮箱" aria-label="聚合所有邮箱" className={`rail-avatar rail-all ${accountFilter === 'all' ? 'active' : ''}`} onClick={() => onSelect()} onContextMenu={(event) => onContextMenu(event)}><Tray size={20} /></button>
      {accounts.map((account) => <button key={account.id} title={`${providerLabel[account.provider]} · ${account.displayName} · ${account.email}`} aria-label={`${providerLabel[account.provider]}，${account.displayName}，${account.email}`} className={`rail-avatar rail-account provider-${account.provider} ${accountFilter === account.id ? 'active' : ''}`} style={{ '--avatar-color': account.color } as CSSProperties} onClick={() => onSelect(account.id)} onContextMenu={(event) => onContextMenu(event, account.id)}><ProviderIcon provider={account.provider} /><span className={`status status-${account.status}`} /></button>)}
      <button title="添加邮箱" aria-label="添加邮箱" className="rail-avatar rail-add" onClick={onAdd}><Plus size={19} /></button>
    </div>
    <button title="设置" aria-label="打开设置" className="rail-avatar rail-settings" onClick={onSettings}><Gear size={19} /></button>
  </aside>;
}
