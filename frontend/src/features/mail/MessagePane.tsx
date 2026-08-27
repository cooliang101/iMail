import { Tag, Tray } from '../../components/icons';
import type { Account, Message } from '../../types';
import type { ParticipantFilters, ParticipantRole } from '../../app-model';
import { AccountProviderMark, providerLabel } from '../../components/shared';
import { VirtualMessageList } from './VirtualMessageList';
import { ParticipantFilterBar } from './ParticipantFilterBar';
import { hasParticipantFilters } from './participant-filter';
import { useI18n } from '../i18n';

export type MailListFilter = 'all' | 'unread' | 'attachments';

export function MessagePane({ title, messageTotal, account, filter, participantFilters, hasOtherFilters, messages, accounts, selectedId, ready, loading, hasMore, onFilterChange, onClearParticipantFilter, onClearParticipantFilters, onKeepOnlyParticipantFilters, onManageLabels, onSelect, onContextMenu, onBackgroundContextMenu, onLoadMore, onAddAccount }: {
  title: string; messageTotal: number; account?: Account; filter: MailListFilter; messages: Message[]; accounts: Account[]; selectedId?: string; ready: boolean; loading: boolean; hasMore: boolean;
  participantFilters: ParticipantFilters; hasOtherFilters: boolean;
  onFilterChange: (filter: MailListFilter) => void; onManageLabels: () => void; onSelect: (id: string) => void; onContextMenu: (message: Message, point: { x: number; y: number }) => void;
  onClearParticipantFilter: (role: ParticipantRole) => void; onClearParticipantFilters: () => void; onKeepOnlyParticipantFilters: () => void;
  onBackgroundContextMenu: (point: { x: number; y: number }) => void; onLoadMore: () => void; onAddAccount: () => void;
}) {
  const { t } = useI18n();
  return <section className="message-pane">
    <div className="pane-title"><div className="pane-heading">
      {account && <AccountProviderMark provider={account.provider} className="pane-provider-mark" />}
      <div className="pane-title-copy"><div className="pane-title-line"><p>{t(title)}</p><span className="pane-count">{t('{count} 封邮件', { count: messageTotal })}</span></div>{account && <span className="pane-subtitle">{providerLabel[account.provider]} · {account.email}</span>}</div>
    </div><button data-icon-tone="info" title={t(selectedId ? '管理所选邮件标签' : '请先选择一封邮件')} aria-label={t('管理邮件标签')} disabled={!selectedId} onClick={onManageLabels}><Tag size={18} /></button></div>
    <div className="message-filters"><button className={filter === 'all' ? 'active' : ''} onClick={() => onFilterChange('all')}>{t('全部')}</button><button className={filter === 'unread' ? 'active' : ''} onClick={() => onFilterChange('unread')}>{t('未读')}</button><button className={filter === 'attachments' ? 'active' : ''} onClick={() => onFilterChange('attachments')}>{t('有附件')}</button></div>
    <ParticipantFilterBar filters={participantFilters} onClear={onClearParticipantFilter} onClearAll={onClearParticipantFilters} />
    <VirtualMessageList messages={messages} accounts={accounts} selectedId={selectedId} ready={ready} loading={loading} hasMore={hasMore} emptyContent={hasParticipantFilters(participantFilters) && accounts.length > 0 ? <div className="empty-state participant-filter-empty"><Tray size={42} weight="duotone" /><h3>{t('当前参与者条件下没有邮件')}</h3><p>{t('可以调整当前搜索条件，或清除参与者筛选。')}</p><div>{hasOtherFilters && <button type="button" onClick={onKeepOnlyParticipantFilters}>{t('只保留参与者条件')}</button>}<button type="button" onClick={onClearParticipantFilters}>{t('清除参与者条件')}</button></div></div> : undefined} onSelect={onSelect} onContextMenu={onContextMenu} onBackgroundContextMenu={onBackgroundContextMenu} onLoadMore={onLoadMore} onAddAccount={onAddAccount} />
  </section>;
}
