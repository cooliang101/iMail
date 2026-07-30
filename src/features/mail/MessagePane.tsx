import { Tag } from '@phosphor-icons/react';
import type { Account, Message } from '../../types';
import { AccountProviderMark, providerLabel } from '../../components/shared';
import { VirtualMessageList } from './VirtualMessageList';

export type MailListFilter = 'all' | 'unread' | 'attachments';

export function MessagePane({ title, messageTotal, account, filter, messages, accounts, selectedId, ready, loading, hasMore, onFilterChange, onManageLabels, onSelect, onContextMenu, onBackgroundContextMenu, onLoadMore, onAddAccount }: {
  title: string; messageTotal: number; account?: Account; filter: MailListFilter; messages: Message[]; accounts: Account[]; selectedId?: string; ready: boolean; loading: boolean; hasMore: boolean;
  onFilterChange: (filter: MailListFilter) => void; onManageLabels: () => void; onSelect: (id: string) => void; onContextMenu: (message: Message, point: { x: number; y: number }) => void;
  onBackgroundContextMenu: (point: { x: number; y: number }) => void; onLoadMore: () => void; onAddAccount: () => void;
}) {
  return <section className="message-pane">
    <div className="pane-title"><div className="pane-heading">
      {account && <AccountProviderMark provider={account.provider} className="pane-provider-mark" />}
      <div className="pane-title-copy"><div className="pane-title-line"><p>{title}</p><span className="pane-count">{messageTotal} 封邮件</span></div>{account && <span className="pane-subtitle">{providerLabel[account.provider]} · {account.email}</span>}</div>
    </div><button data-icon-tone="info" title={selectedId ? '管理所选邮件标签' : '请先选择一封邮件'} aria-label="管理邮件标签" disabled={!selectedId} onClick={onManageLabels}><Tag size={18} /></button></div>
    <div className="message-filters"><button className={filter === 'all' ? 'active' : ''} onClick={() => onFilterChange('all')}>全部</button><button className={filter === 'unread' ? 'active' : ''} onClick={() => onFilterChange('unread')}>未读</button><button className={filter === 'attachments' ? 'active' : ''} onClick={() => onFilterChange('attachments')}>有附件</button></div>
    <VirtualMessageList messages={messages} accounts={accounts} selectedId={selectedId} ready={ready} loading={loading} hasMore={hasMore} onSelect={onSelect} onContextMenu={onContextMenu} onBackgroundContextMenu={onBackgroundContextMenu} onLoadMore={onLoadMore} onAddAccount={onAddAccount} />
  </section>;
}
