import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button } from '@fluentui/react-components';
import { Archive, ArrowClockwise, ArrowRight, Bell, CaretDown, Check, Clock, Code, FolderSimplePlus, Gear, Tray, MagnifyingGlass, PaperPlaneTilt, PencilSimple, Plus, SidebarSimple, Star, Tag, UserCircle, WarningCircle, X } from '@phosphor-icons/react';
import { api } from './api';
import type { Account, DeveloperToken, Draft, MailboxRole, Message } from './types';
import type { MailNotification, Notice } from './app-model';
import { AccountProviderMark, ProviderIcon, providerLabel } from './components/shared';
import { AppInput } from './components/form-controls';
import { VirtualMessageList, MessageReader } from './features/mail';
import { AddAccountModal, AccountSettingsModal } from './features/accounts';
import { ComposePane, DraftWorkspace } from './features/compose';
import { LabelModal, NotificationsModal, SnoozeModal, WorkspaceFolderItem, WorkspaceIcon, WorkspaceModal, type WorkspaceFolder } from './features/organize';
import { CreateTokenModal, TokenWorkspace } from './features/developer';

type View = 'inbox' | 'starred' | 'sent' | 'snoozed' | 'archive' | 'folder' | 'drafts' | 'tokens';
type MessagePage = { messages: Message[]; total: number; nextOffset: number; hasMore: boolean };
type MessageStats = {
  total: number;
  unread: number;
  byAccount: Array<{ accountId: string; total: number; unread: number }>;
  byGroup: Array<{ group: string; total: number; unread: number }>;
};

function App() {
  const [realAccounts, setRealAccounts] = useState<Account[]>([]);
  const [realMessages, setRealMessages] = useState<Message[]>([]);
  const [tokens, setTokens] = useState<DeveloperToken[]>([]);
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [labels, setLabels] = useState<string[]>([]);
  const [ready, setReady] = useState(false);
  const [view, setView] = useState<View>('inbox');
  const [accountFilter, setAccountFilter] = useState('all');
  const [groupFilter, setGroupFilter] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [mailFilter, setMailFilter] = useState<'all' | 'unread' | 'attachments'>('all');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [composeMode, setComposeMode] = useState<'new' | 'reply' | 'forward' | null>(null);
  const [tokenOpen, setTokenOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [notificationsOpen, setNotificationsOpen] = useState(false);
  const [notifications, setNotifications] = useState<MailNotification[]>([]);
  const [labelOpen, setLabelOpen] = useState(false);
  const [snoozeOpen, setSnoozeOpen] = useState(false);
  const [workspaceOpen, setWorkspaceOpen] = useState<string | null | undefined>(undefined);
  const [activeMailbox, setActiveMailbox] = useState<WorkspaceFolder | null>(null);
  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Set<string>>(() => new Set());
  const [activeLabel, setActiveLabel] = useState<string | null>(null);
  const [activeDraft, setActiveDraft] = useState<Draft | undefined>();
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  const [syncing, setSyncing] = useState(false);
  const [messageTotal, setMessageTotal] = useState(0);
  const [messagesHasMore, setMessagesHasMore] = useState(false);
  const [messagesLoading, setMessagesLoading] = useState(false);
  const [messageActionBusy, setMessageActionBusy] = useState(false);
  const [messageRevision, setMessageRevision] = useState(0);
  const [messageStats, setMessageStats] = useState<MessageStats>({ total: 0, unread: 0, byAccount: [], byGroup: [] });
  const messageQueryRef = useRef('');
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const folderDiscoveryStarted = useRef(false);

  const accounts = realAccounts;
  const messages = realMessages;

  async function load() {
    try {
      const [accountData, tokenData, statsData, draftData, labelData] = await Promise.all([
        api<{ accounts: Account[] }>('/api/accounts'),
        api<{ tokens: DeveloperToken[] }>('/api/developer-tokens'),
        api<MessageStats>('/api/message-stats'),
        api<{ drafts: Draft[] }>('/api/drafts'),
        api<{ labels: string[] }>('/api/labels'),
      ]);
      setRealAccounts(accountData.accounts);
      setTokens(tokenData.tokens);
      setMessageStats(statsData);
      setDrafts(draftData.drafts);
      setLabels(labelData.labels);
    } catch (error) {
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '服务连接失败' });
    }
  }

  useEffect(() => { void load(); }, []);
  useEffect(() => {
    if (folderDiscoveryStarted.current || accounts.length === 0 || accounts.some((account) => account.mailboxes.length > 0)) return;
    folderDiscoveryStarted.current = true;
    void api('/api/sync', { method: 'POST' }).then(load).catch(() => undefined);
  }, [accounts]);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 4200);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const groups = useMemo(() => Array.from(new Set(accounts.map((account) => account.group))), [accounts]);
  const workspaceFolders = useMemo(() => new Map(groups.map((group) => {
    const merged = new Map<string, WorkspaceFolder>();
    for (const account of accounts.filter((item) => item.group === group)) {
      for (const mailbox of account.mailboxes.filter((item) => item.selectable && !['\\Inbox', '\\Sent', '\\Archive', '\\All'].includes(item.specialUse ?? ''))) {
        const key = mailbox.name.toLocaleLowerCase();
        const current = merged.get(key) ?? { group, name: mailbox.name, unread: 0, targets: [] };
        current.unread += mailbox.unread ?? 0;
        current.targets.push({ accountId: account.id, accountName: account.displayName, path: mailbox.path });
        merged.set(key, current);
      }
    }
    return [group, Array.from(merged.values()).sort((left, right) => left.name.localeCompare(right.name, 'zh-CN'))] as const;
  })), [accounts, groups]);
  const messageQuery = useMemo(() => {
    const params = new URLSearchParams();
    if (accountFilter !== 'all') params.set('accountId', accountFilter);
    if (groupFilter) params.set('group', groupFilter);
    if (search.trim()) params.set('q', search.trim());
    if (view === 'starred') params.set('flagged', 'true');
    if (view === 'folder' && activeMailbox) { params.set('group', activeMailbox.group); params.set('mailboxName', activeMailbox.name); }
    else {
      const mailboxRole: MailboxRole = view === 'sent' ? 'sent' : view === 'archive' ? 'archive' : 'inbox';
      params.set('mailboxRole', mailboxRole);
    }
    if (view === 'snoozed') params.set('snoozed', 'true');
    if (activeLabel) params.set('label', activeLabel);
    if (mailFilter === 'unread') params.set('unread', 'true');
    if (mailFilter === 'attachments') params.set('hasAttachments', 'true');
    return params.toString();
  }, [accountFilter, groupFilter, search, view, mailFilter, activeLabel, activeMailbox]);
  const selected = messages.find((message) => message.id === selectedId) ?? messages[0];
  const selectedIndex = selected ? messages.findIndex((message) => message.id === selected.id) : -1;
  const activeAccount = accountFilter === 'all' ? undefined : accounts.find((account) => account.id === accountFilter);
  const visibleMessages = mailFilter === 'unread' ? messages.filter((message) => message.unread) : messages;

  useEffect(() => {
    const focusSearch = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); searchInputRef.current?.focus(); }
    };
    window.addEventListener('keydown', focusSearch);
    return () => window.removeEventListener('keydown', focusSearch);
  }, []);

  useEffect(() => {
    let cancelled = false;
    messageQueryRef.current = messageQuery;
    const timer = window.setTimeout(() => {
      setMessagesLoading(true);
      void api<MessagePage>(`/api/messages?${messageQuery}&limit=60&offset=0`).then((result) => {
        if (cancelled) return;
        setRealMessages(result.messages); setMessageTotal(result.total); setMessagesHasMore(result.hasMore); setSelectedId(null);
      }).catch((error) => { if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮件缓存加载失败' }); })
        .finally(() => { if (!cancelled) { setMessagesLoading(false); setReady(true); } });
    }, search.trim() ? 220 : 0);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [messageQuery, messageRevision]);

  useEffect(() => {
    if ((view !== 'sent' && view !== 'archive') || accounts.length === 0) return;
    let cancelled = false; setSyncing(true);
    void api(`/api/mailboxes/${view}/sync`, { method: 'POST' }).then(() => {
      if (!cancelled) { void load(); setMessageRevision((value) => value + 1); }
    }).catch((error) => { if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '文件夹同步失败' }); })
      .finally(() => { if (!cancelled) setSyncing(false); });
    return () => { cancelled = true; };
  }, [view]);

  useEffect(() => {
    if (view !== 'folder' || !activeMailbox) return;
    let cancelled = false; setSyncing(true);
    void Promise.all(activeMailbox.targets.map((target) => api(`/api/accounts/${target.accountId}/mailboxes/sync`, { method: 'POST', body: JSON.stringify({ mailbox: target.path }) }))).then(() => {
      if (!cancelled) { void load(); setMessageRevision((value) => value + 1); }
    }).catch((error) => { if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '文件夹同步失败' }); })
      .finally(() => { if (!cancelled) setSyncing(false); });
    return () => { cancelled = true; };
  }, [view, activeMailbox]);

  useEffect(() => {
    if (!selected || selected.text !== undefined) return;
    let cancelled = false;
    void api<{ message: Message }>(`/api/messages/${selected.id}`).then(({ message }) => {
      if (!cancelled) setRealMessages((current) => current.map((item) => item.id === message.id
        ? { ...message, unread: item.unread, flagged: item.flagged }
        : item));
    }).catch((error) => { if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮件正文加载失败' }); });
    return () => { cancelled = true; };
  }, [selected?.id, selected?.text]);

  const loadMoreMessages = useCallback(async () => {
    if (messagesLoading || !messagesHasMore) return;
    const queryAtStart = messageQuery;
    setMessagesLoading(true);
    try {
      const offset = mailFilter === 'unread' ? realMessages.filter((message) => message.unread).length : realMessages.length;
      const result = await api<MessagePage>(`/api/messages?${queryAtStart}&limit=60&offset=${offset}`);
      if (messageQueryRef.current !== queryAtStart) return;
      setRealMessages((current) => [...current, ...result.messages.filter((message) => !current.some((item) => item.id === message.id))]);
      setMessageTotal(result.total); setMessagesHasMore(result.hasMore);
    } catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '加载更多邮件失败' }); }
    finally { setMessagesLoading(false); }
  }, [mailFilter, messageQuery, messagesHasMore, messagesLoading, realMessages]);

  async function syncAll() {
    if (realAccounts.length === 0) { setAddOpen(true); return; }
    setSyncing(true);
    try {
      const role = view === 'sent' ? 'sent' : view === 'archive' ? 'archive' : view === 'inbox' || view === 'starred' || view === 'snoozed' ? 'inbox' : null;
      if (view === 'folder' && activeMailbox) await Promise.all(activeMailbox.targets.map((target) => api(`/api/accounts/${target.accountId}/mailboxes/sync`, { method: 'POST', body: JSON.stringify({ mailbox: target.path }) })));
      else if (role) await api(role === 'inbox' ? '/api/sync' : `/api/mailboxes/${role}/sync`, { method: 'POST' });
      await load(); setMessageRevision((value) => value + 1); setNotice({ kind: 'success', text: '缓存已更新' });
    }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '同步失败' }); }
    finally { setSyncing(false); }
  }

  async function openNotifications() {
    try { const result = await api<{ notifications: MailNotification[] }>('/api/notifications'); setNotifications(result.notifications); setNotificationsOpen(true); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '通知加载失败' }); }
  }

  async function updateSelectedLocal(input: { labels?: string[]; snoozedUntil?: string | null }, success: string) {
    if (!selected) return;
    try {
      const result = await api<{ message: Message }>(`/api/messages/${selected.id}`, { method: 'PATCH', body: JSON.stringify(input) });
      setRealMessages((current) => current.map((item) => item.id === selected.id ? { ...item, ...result.message } : item));
      await load(); setMessageRevision((value) => value + 1); setNotice({ kind: 'success', text: success });
    } catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮件更新失败' }); }
  }

  async function markSelectedUnread() {
    if (!selected || selected.unread) return;
    try { await api(`/api/messages/${selected.id}`, { method: 'PATCH', body: JSON.stringify({ unread: true }) }); setRealMessages((current) => current.map((item) => item.id === selected.id ? { ...item, unread: true } : item)); if (selected.mailboxRole === 'inbox') adjustMessageStats(selected.accountId, 0, 1); setNotice({ kind: 'success', text: '邮件已标记为未读' }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '未读状态更新失败' }); }
  }

  async function toggleSelectedFlag() {
    if (!selected) return;
    const flagged = !selected.flagged;
    setRealMessages((current) => current.map((message) => message.id === selected.id ? { ...message, flagged } : message));
    try {
      await api(`/api/messages/${selected.id}`, { method: 'PATCH', body: JSON.stringify({ flagged }) });
      if (view === 'starred' && !flagged) setMessageRevision((value) => value + 1);
    } catch (error) {
      setRealMessages((current) => current.map((message) => message.id === selected.id ? { ...message, flagged: !flagged } : message));
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '星标更新失败' });
    }
  }

  function adjustMessageStats(accountId: string, totalDelta: number, unreadDelta: number) {
    const account = accounts.find((item) => item.id === accountId);
    setMessageStats((current) => ({
      ...current,
      total: Math.max(0, current.total + totalDelta),
      unread: Math.max(0, current.unread + unreadDelta),
      byAccount: current.byAccount.map((item) => item.accountId === accountId ? { ...item, total: Math.max(0, item.total + totalDelta), unread: Math.max(0, item.unread + unreadDelta) } : item),
      byGroup: current.byGroup.map((item) => item.group === account?.group ? { ...item, total: Math.max(0, item.total + totalDelta), unread: Math.max(0, item.unread + unreadDelta) } : item),
    }));
  }

  function selectMessage(id: string) {
    setSelectedId(id);
    const message = realMessages.find((item) => item.id === id);
    if (!message?.unread) return;

    setRealMessages((current) => current.map((item) => item.id === id ? { ...item, unread: false } : item));
    if (mailFilter === 'unread') setMessageTotal((current) => Math.max(0, current - 1));
    if (message.mailboxRole === 'inbox') adjustMessageStats(message.accountId, 0, -1);
    void api(`/api/messages/${id}`, { method: 'PATCH', body: JSON.stringify({ unread: false }) }).catch((error) => {
      setRealMessages((current) => current.map((item) => item.id === id ? { ...item, unread: true } : item));
      if (mailFilter === 'unread') setMessageTotal((current) => current + 1);
      if (message.mailboxRole === 'inbox') adjustMessageStats(message.accountId, 0, 1);
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮件已读状态更新失败' });
    });
  }

  async function moveSelected(destination: 'archive' | 'trash') {
    if (!selected || messageActionBusy) return;
    const message = selected;
    const index = messages.findIndex((item) => item.id === message.id);
    const nextId = messages[index + 1]?.id ?? messages[index - 1]?.id ?? null;
    const unreadDelta = message.unread ? -1 : 0;
    setMessageActionBusy(true);
    setRealMessages((current) => current.filter((item) => item.id !== message.id));
    setMessageTotal((current) => Math.max(0, current - 1));
    if (message.mailboxRole === 'inbox') adjustMessageStats(message.accountId, -1, unreadDelta);
    setSelectedId(nextId);
    try {
      await api(`/api/messages/${message.id}/move`, { method: 'POST', body: JSON.stringify({ destination }) });
      setNotice({ kind: 'success', text: destination === 'archive' ? '邮件已归档' : '邮件已移至垃圾箱' });
    } catch (error) {
      setRealMessages((current) => {
        if (current.some((item) => item.id === message.id)) return current;
        const restored = [...current]; restored.splice(Math.min(index, restored.length), 0, message); return restored;
      });
      setMessageTotal((current) => current + 1);
      if (message.mailboxRole === 'inbox') adjustMessageStats(message.accountId, 1, -unreadDelta);
      setSelectedId(message.id);
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮件移动失败' });
    } finally {
      setMessageActionBusy(false);
    }
  }

  function selectScope(nextView: View, nextAccount = 'all', nextGroup: string | null = null) {
    setView(nextView); setAccountFilter(nextAccount); setGroupFilter(nextGroup); setActiveLabel(null); setActiveMailbox(null); setSidebarOpen(false); setSelectedId(null);
  }

  function selectMailbox(folder: WorkspaceFolder) {
    setView('folder'); setAccountFilter('all'); setGroupFilter(null); setActiveLabel(null); setActiveMailbox(folder); setSidebarOpen(false); setSelectedId(null);
  }

  function selectLabel(label: string) { setView('inbox'); setAccountFilter('all'); setGroupFilter(null); setActiveLabel(label); setSidebarOpen(false); setSelectedId(null); }

  const scopeTitle = activeMailbox?.name ?? (activeLabel ? `标签 · ${activeLabel}` : view === 'starred' ? '星标邮件' : view === 'sent' ? '已发送' : view === 'snoozed' ? '稍后处理' : view === 'archive' ? '归档' : '统一收件箱');

  return <div className={`app-shell ${sidebarCollapsed ? 'sidebar-collapsed' : ''}`}>
    {notice && <div className={`toast toast-${notice.kind}`}>{notice.kind === 'success' ? <Check size={18} /> : <WarningCircle size={18} />}<span>{notice.text}</span></div>}

    <aside className="account-rail" aria-label="邮箱账户">
      <button className="brand-mark" aria-label="iMail"><img src="/brand/imail-app-icon.png" alt="" /></button>
      <div className="rail-accounts">
        <button title="聚合所有邮箱" aria-label="聚合所有邮箱" className={`rail-avatar rail-all ${accountFilter === 'all' ? 'active' : ''}`} onClick={() => selectScope('inbox')}><Tray size={20} /></button>
        {accounts.map((account) =>
          <button key={account.id} title={`${providerLabel[account.provider]} · ${account.displayName} · ${account.email}`} aria-label={`${providerLabel[account.provider]}，${account.displayName}，${account.email}`} className={`rail-avatar rail-account provider-${account.provider} ${accountFilter === account.id ? 'active' : ''}`} style={{ '--avatar-color': account.color } as React.CSSProperties} onClick={() => selectScope('inbox', account.id)}>
            <ProviderIcon provider={account.provider} /><span className={`status status-${account.status}`} />
          </button>)}
        <button title="添加邮箱" aria-label="添加邮箱" className="rail-avatar rail-add" onClick={() => setAddOpen(true)}><Plus size={19} /></button>
      </div>
      <button title="邮箱设置" aria-label="邮箱设置" className="rail-avatar rail-settings" onClick={() => setSettingsOpen(true)}><Gear size={19} /></button>
    </aside>

    <aside className={`primary-sidebar ${sidebarOpen ? 'mobile-open' : ''}`}>
      <div className="sidebar-heading"><div><strong>iMail</strong><span>统一通信工作台</span></div><button className="mobile-close" aria-label="关闭侧栏" onClick={() => setSidebarOpen(false)}><X size={20} /></button></div>
      <Button appearance="primary" icon={<PencilSimple size={18} />} className="compose-button" onClick={() => { setActiveDraft(undefined); setComposeMode('new'); setView('inbox'); setSidebarOpen(false); }}>写邮件</Button>
      <div className="mobile-account-controls" aria-label="移动端邮箱账户">
        <button className={accountFilter === 'all' ? 'active' : ''} onClick={() => selectScope('inbox')}><Tray size={18} /><span><strong>全部邮箱</strong><small>{accounts.length} 个账户</small></span></button>
        {accounts.map((account) => <button key={account.id} className={accountFilter === account.id ? 'active' : ''} onClick={() => selectScope('inbox', account.id)}><AccountProviderMark provider={account.provider} /><span><strong>{account.displayName}</strong><small>{account.email}</small></span></button>)}
        <div><button onClick={() => { setAddOpen(true); setSidebarOpen(false); }}><Plus size={16} />添加邮箱</button><button onClick={() => { setSettingsOpen(true); setSidebarOpen(false); }}><Gear size={16} />邮箱设置</button></div>
      </div>
      <nav className="nav-block">
        <button data-icon-tone="primary" className={view === 'inbox' && !groupFilter ? 'active' : ''} onClick={() => selectScope('inbox')}><Tray size={19} /><span>统一收件箱</span><b>{messageStats.unread || ''}</b></button>
        <button data-icon-tone="warning" className={view === 'starred' ? 'active' : ''} onClick={() => selectScope('starred')}><Star size={19} /><span>已加星标</span></button>
        <button data-icon-tone="info" className={view === 'sent' ? 'active' : ''} onClick={() => selectScope('sent')}><PaperPlaneTilt size={19} /><span>已发送</span></button>
        <button data-icon-tone="accent" className={view === 'drafts' ? 'active' : ''} onClick={() => selectScope('drafts')}><PencilSimple size={19} /><span>草稿</span><b>{drafts.length || ''}</b></button>
        <button data-icon-tone="warning" className={view === 'snoozed' ? 'active' : ''} onClick={() => selectScope('snoozed')}><Clock size={19} /><span>稍后处理</span></button>
        <button data-icon-tone="neutral" className={view === 'archive' ? 'active' : ''} onClick={() => selectScope('archive')}><Archive size={19} /><span>归档</span></button>
      </nav>
      <section className="workspace-section"><div className="section-label"><span>工作空间</span><button className="workspace-add" title="新增或整理工作空间" aria-label="新增工作空间" onClick={() => setWorkspaceOpen(null)}><FolderSimplePlus size={16} /></button></div>
        <nav className="nav-block groups workspace-list">
          {groups.map((group) => {
            const folders = workspaceFolders.get(group) ?? [];
            const expanded = expandedWorkspaces.has(group);
            const visibleFolders = expanded ? folders : folders.slice(0, 3);
            const icon = accounts.find((account) => account.group === group)?.groupIcon ?? 'folder';
            return <div className="workspace-group" key={group}>
              <div className="workspace-row"><button className={groupFilter === group ? 'active' : ''} onClick={() => selectScope('inbox', 'all', group)}><WorkspaceIcon icon={icon} size={17} /><span>{group}</span><b>{messageStats.byGroup.find((item) => item.group === group)?.unread || ''}</b></button><button className="workspace-edit" title={`编辑工作空间 ${group}`} aria-label={`编辑工作空间 ${group}`} onClick={() => setWorkspaceOpen(group)}><PencilSimple size={14} /></button></div>
              <div className="workspace-mailboxes">{visibleFolders.map((folder) => <WorkspaceFolderItem key={folder.name.toLocaleLowerCase()} folder={folder} active={view === 'folder' && activeMailbox?.group === group && activeMailbox.name.toLocaleLowerCase() === folder.name.toLocaleLowerCase()} onSelect={selectMailbox} />)}{folders.length > 3 && <button className={`workspace-folder-toggle ${expanded ? 'is-expanded' : ''}`} onClick={() => setExpandedWorkspaces((current) => { const next = new Set(current); if (expanded) next.delete(group); else next.add(group); return next; })}><CaretDown size={14} /><span>{expanded ? '收起' : `更多 ${folders.length - 3}`}</span></button>}</div>
            </div>;
          })}
        </nav>
      </section>
      {labels.length > 0 && <><div className="section-label"><span>邮件标签</span></div><nav className="nav-block groups label-nav">{labels.map((label) => <button key={label} data-icon-tone="info" className={activeLabel === label ? 'active' : ''} onClick={() => selectLabel(label)}><Tag size={16} /><span>{label}</span></button>)}</nav></>}
      <div className="sidebar-spacer" />
      <button className={`developer-entry ${view === 'tokens' ? 'active' : ''}`} onClick={() => selectScope('tokens')}><Code size={19} /><span><strong>开发者网关</strong><small>Token 与邮件 API</small></span><ArrowRight size={16} /></button>
      <div className="user-strip"><UserCircle size={32} weight="duotone" /><span><strong>本地工作区</strong><small>数据仅存储在本机</small></span><CaretDown size={15} /></div>
    </aside>

    <main className="workspace">
      <header className="topbar">
        <button className="sidebar-trigger desktop-sidebar-trigger" title={sidebarCollapsed ? '展开侧栏' : '收起侧栏'} aria-label={sidebarCollapsed ? '展开侧栏' : '收起侧栏'} aria-expanded={!sidebarCollapsed} onClick={() => setSidebarCollapsed((current) => !current)}><SidebarSimple size={20} /></button>
        <button className="sidebar-trigger mobile-sidebar-trigger" title="打开侧栏" aria-label="打开侧栏" aria-expanded={sidebarOpen} onClick={() => setSidebarOpen(true)}><SidebarSimple size={20} /></button>
        <AppInput className="search-box" contentBefore={<MagnifyingGlass size={18} />} contentAfter={<kbd>Ctrl K</kbd>} ref={searchInputRef} value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索当前范围内的邮件" aria-label="搜索当前范围内的邮件" />
        <button data-icon-tone="primary" className={`sync-button ${syncing ? 'is-syncing' : ''}`} onClick={() => void syncAll()}><ArrowClockwise size={18} /><span>{syncing ? '同步中' : '同步'}</span></button>
        <button data-icon-tone="info" className="icon-button" title="通知中心" aria-label="打开通知中心" onClick={() => void openNotifications()}><Bell size={19} /></button>
      </header>

      {view === 'tokens' ? <TokenWorkspace accounts={realAccounts} tokens={tokens} onCreate={() => setTokenOpen(true)} onReload={load} setNotice={setNotice} /> :
        <div className={`mail-layout ${selectedId || composeMode ? 'mobile-reader-open' : ''}`}>
          {view === 'drafts' ? <DraftWorkspace drafts={drafts} accounts={accounts} onOpen={(draft) => { setActiveDraft(draft); setComposeMode('new'); }} onDelete={async (id) => { try { await api(`/api/drafts/${id}`, { method: 'DELETE' }); setDrafts((current) => current.filter((draft) => draft.id !== id)); if (activeDraft?.id === id) { setActiveDraft(undefined); setComposeMode(null); } setNotice({ kind: 'success', text: '草稿已删除' }); } catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '草稿删除失败' }); } }} onCreate={() => { setActiveDraft(undefined); setComposeMode('new'); }} /> : <section className="message-pane">
            <div className="pane-title">
              <div className="pane-heading">
                {activeAccount && <AccountProviderMark provider={activeAccount.provider} className="pane-provider-mark" />}
                <div className="pane-title-copy">
                  <div className="pane-title-line"><p>{groupFilter ?? (accountFilter === 'all' ? scopeTitle : activeAccount?.displayName)}</p><span className="pane-count">{messageTotal} 封邮件</span></div>
                  {activeAccount && <span className="pane-subtitle">{providerLabel[activeAccount.provider]} · {activeAccount.email}</span>}
                </div>
              </div>
              <button data-icon-tone="info" title={selected ? '管理所选邮件标签' : '请先选择一封邮件'} aria-label="管理邮件标签" disabled={!selected} onClick={() => setLabelOpen(true)}><Tag size={18} /></button>
            </div>
            <div className="message-filters"><button className={mailFilter === 'all' ? 'active' : ''} onClick={() => setMailFilter('all')}>全部</button><button className={mailFilter === 'unread' ? 'active' : ''} onClick={() => setMailFilter('unread')}>未读</button><button className={mailFilter === 'attachments' ? 'active' : ''} onClick={() => setMailFilter('attachments')}>有附件</button></div>
            <VirtualMessageList messages={visibleMessages} accounts={accounts} selectedId={selected?.id} ready={ready} loading={messagesLoading} hasMore={messagesHasMore} onSelect={selectMessage} onLoadMore={loadMoreMessages} onAddAccount={() => setAddOpen(true)} />
          </section>}
          {composeMode ? <ComposePane key={`${composeMode}-${activeDraft?.id ?? selected?.id ?? 'new'}`} accounts={realAccounts} mode={composeMode} original={composeMode === 'new' ? undefined : selected} draft={activeDraft} onClose={() => { setComposeMode(null); setActiveDraft(undefined); }} onDraftSaved={(saved) => { setDrafts((current) => [saved, ...current.filter((item) => item.id !== saved.id)]); }} onSent={async () => { setComposeMode(null); setActiveDraft(undefined); await load(); setNotice({ kind: 'success', text: '邮件已发送' }); }} /> : view === 'drafts' ? <section className="composer-pane composer-welcome"><PencilSimple size={48} weight="duotone" /><h2>选择草稿继续编辑</h2><p>修改会自动保存，也可以直接新建一封邮件。</p><button onClick={() => { setActiveDraft(undefined); setComposeMode('new'); }}>新建邮件</button></section> : <MessageReader message={selected} account={selected ? accounts.find((item) => item.id === selected.accountId) : undefined} onReply={() => { setActiveDraft(undefined); setComposeMode('reply'); }} onForward={() => { setActiveDraft(undefined); setComposeMode('forward'); }} onCloseMobile={() => setSelectedId(null)}
            onToggleFlag={() => void toggleSelectedFlag()}
            onSnooze={() => setSnoozeOpen(true)} onManageLabels={() => setLabelOpen(true)} onMarkUnread={() => void markSelectedUnread()}
            onArchive={() => void moveSelected('archive')} onDelete={() => void moveSelected('trash')} actionBusy={messageActionBusy}
            onPrevious={() => { if (selectedIndex > 0) selectMessage(messages[selectedIndex - 1].id); }}
            onNext={() => { if (selectedIndex >= 0 && selectedIndex < messages.length - 1) selectMessage(messages[selectedIndex + 1].id); }}
            hasPrevious={selectedIndex > 0} hasNext={selectedIndex >= 0 && selectedIndex < messages.length - 1} />}
        </div>}
    </main>

    {addOpen && <AddAccountModal accounts={accounts} onClose={() => setAddOpen(false)} onAdded={async (result) => { setAddOpen(false); await load(); setMessageRevision((value) => value + 1); setNotice(result?.warning ? { kind: 'error', text: `授权已保存，连接验证失败：${result.warning}` } : { kind: 'success', text: '邮箱已接入，正在准备统一收件箱' }); }} />}
    {tokenOpen && <CreateTokenModal accounts={realAccounts} onClose={() => setTokenOpen(false)} onCreated={async () => { await load(); }} />}
    {settingsOpen && <AccountSettingsModal accounts={realAccounts} onClose={() => setSettingsOpen(false)} onReload={load} setNotice={setNotice} />}
    {notificationsOpen && <NotificationsModal notifications={notifications} accounts={accounts} onClose={() => setNotificationsOpen(false)} onOpenMessage={(notification) => { setNotificationsOpen(false); if (notification.accountId) setAccountFilter(notification.accountId); setView('inbox'); setSelectedId(notification.messageId ?? null); }} />}
    {labelOpen && selected && <LabelModal message={selected} knownLabels={labels} onClose={() => setLabelOpen(false)} onSave={(next) => { setLabelOpen(false); void updateSelectedLocal({ labels: next }, '邮件标签已更新'); }} />}
    {snoozeOpen && selected && <SnoozeModal onClose={() => setSnoozeOpen(false)} onSave={(until) => { setSnoozeOpen(false); void updateSelectedLocal({ snoozedUntil: until }, until ? '邮件已移到稍后处理' : '邮件已返回收件箱'); }} />}
    {workspaceOpen !== undefined && <WorkspaceModal accounts={accounts} workspace={workspaceOpen ?? undefined} onClose={() => setWorkspaceOpen(undefined)} onSaved={async () => { setWorkspaceOpen(undefined); await load(); setNotice({ kind: 'success', text: '工作空间已更新' }); }} />}
  </div>;
}

export default App;
