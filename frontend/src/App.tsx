import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { ArrowCounterClockwise, Check, WarningCircle } from '@phosphor-icons/react';
import { api } from './api';
import { buildWorkspaceFolders } from './app-selectors';
import type { Account, Contact, DeveloperToken, Draft, Message } from './types';
import type { AppView, ContextTarget, MailNotification, MessageStats, Notice, WorkspaceFolder } from './app-model';
import { AppAccountRail, AppSidebar, AppTopbar, useWorkspaceNavigation } from './features/navigation';
import { applyOptimisticMessageMutation, MessageActionCoordinator, MessagePane, MessageReader, rollbackOptimisticMessageMutation, type MailListFilter, useMessageCollection } from './features/mail';
import type { ComposePaneHandle } from './features/compose/ComposePane';
import { isEditableShortcutTarget, shortcutDefinitions, shortcutLabel, shortcutMatches } from './features/shortcuts';
import type { SettingsTab } from './features/settings/SettingsModal';
import { useAppPreferences } from './features/settings/useAppPreferences';
import { useAuth } from './features/auth';
import { useAppTheme } from './features/appearance';
import { subscribeDesktopAccountSelection, subscribeDesktopCompose, updateDesktopTrayMenu } from './platform/desktop-events';
import { useNewMailNotifications } from './features/notifications';
import { FeatureErrorBoundary, WorkspaceErrorBoundary } from './components/ErrorBoundary';
import { desktopLog, describeDesktopLogValue } from './desktop-logging';

const AddAccountModal = lazy(() => import('./features/accounts/AddAccountModal').then((module) => ({ default: module.AddAccountModal })));
const ComposePane = lazy(() => import('./features/compose/ComposePane').then((module) => ({ default: module.ComposePane })));
const ContactsWorkspace = lazy(() => import('./features/contacts/ContactsWorkspace').then((module) => ({ default: module.ContactsWorkspace })));
const CreateApiTokenModal = lazy(() => import('./features/developer/CreateApiTokenModal').then((module) => ({ default: module.CreateApiTokenModal })));
const CreateMcpTokenModal = lazy(() => import('./features/developer/CreateMcpTokenModal').then((module) => ({ default: module.CreateMcpTokenModal })));
const DraftWelcome = lazy(() => import('./features/compose/DraftWelcome').then((module) => ({ default: module.DraftWelcome })));
const DraftWorkspace = lazy(() => import('./features/compose/DraftWorkspace').then((module) => ({ default: module.DraftWorkspace })));
const LabelModal = lazy(() => import('./features/organize/LabelModal').then((module) => ({ default: module.LabelModal })));
const NotificationsModal = lazy(() => import('./features/organize/NotificationsModal').then((module) => ({ default: module.NotificationsModal })));
const SettingsModal = lazy(() => import('./features/settings/SettingsModal').then((module) => ({ default: module.SettingsModal })));
const PreferencesSyncErrorDialog = lazy(() => import('./features/settings/PreferencesSyncErrorDialog').then((module) => ({ default: module.PreferencesSyncErrorDialog })));
const SnoozeModal = lazy(() => import('./features/organize/SnoozeModal').then((module) => ({ default: module.SnoozeModal })));
const TokenWorkspace = lazy(() => import('./features/developer/TokenWorkspace').then((module) => ({ default: module.TokenWorkspace })));
const WorkspaceModal = lazy(() => import('./features/organize/WorkspaceModal').then((module) => ({ default: module.WorkspaceModal })));
const AppContextMenu = lazy(() => import('./features/context-menu/AppContextMenu').then((module) => ({ default: module.AppContextMenu })));

function FeatureFallback({ label }: { label: string }) {
  return <div className="feature-loading" role="status">正在加载{label}…</div>;
}

type PendingMove = { message: Message; index: number; nextId: string | null; destination: 'archive' | 'trash'; unreadDelta: number };

function App() {
  const { user, logout } = useAuth();
  const { setTheme } = useAppTheme();
  const [realAccounts, setRealAccounts] = useState<Account[]>([]);
  const [tokens, setTokens] = useState<DeveloperToken[]>([]);
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [contacts, setContacts] = useState<Contact[]>([]);
  const [labels, setLabels] = useState<string[]>([]);
  const [notice, setNotice] = useState<Notice>(null);
  const { preferences, shortcutBindings, preferencesSyncIssue, dismissPreferencesSyncIssue, savePreferences, saveShortcutBindings } = useAppPreferences(user.id, setNotice);
  const { view, setView, accountFilter, setAccountFilter, groupFilter, setGroupFilter, search, setSearch, activeLabel, activeMailbox, sidebarOpen, setSidebarOpen, sidebarCollapsed, setSidebarCollapsed, selectScope: selectNavigationScope, selectMailbox: selectNavigationMailbox, selectLabel: selectNavigationLabel } = useWorkspaceNavigation(preferences.startupView);
  const [mailFilter, setMailFilter] = useState<MailListFilter>('all');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [composeMode, setComposeMode] = useState<'new' | 'reply' | 'forward' | null>(null);
  const [tokenOpen, setTokenOpen] = useState<'api' | 'mcp' | null>(null);
  const [settingsTab, setSettingsTab] = useState<SettingsTab | null>(null);
  const [contextTarget, setContextTarget] = useState<ContextTarget | null>(null);
  const [notificationsOpen, setNotificationsOpen] = useState(false);
  const [notifications, setNotifications] = useState<MailNotification[]>([]);
  const [labelOpen, setLabelOpen] = useState(false);
  const [snoozeOpen, setSnoozeOpen] = useState(false);
  const [workspaceOpen, setWorkspaceOpen] = useState<string | null | undefined>(undefined);
  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Set<string>>(() => new Set());
  const [activeDraft, setActiveDraft] = useState<Draft | undefined>();
  const [composeAccountId, setComposeAccountId] = useState<string | undefined>();
  const [composeInitialTo, setComposeInitialTo] = useState<string[]>([]);
  const [messageActionBusy, setMessageActionBusy] = useState(false);
  const [pendingMove, setPendingMove] = useState<PendingMove | null>(null);
  const realAccountsRef = useRef<Account[]>([]);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const folderDiscoveryStarted = useRef(false);
  const composePaneRef = useRef<ComposePaneHandle | null>(null);
  const messageActionsRef = useRef(new MessageActionCoordinator());

  const accounts = realAccounts;
  const { messages, setMessages: setRealMessages, messageTotal, setMessageTotal, hasMore: messagesHasMore, loading: messagesLoading, ready, setRevision: setMessageRevision, stats: messageStats, setStats: setMessageStats, selected, loadMore: loadMoreMessages } = useMessageCollection({
    accounts,
    view,
    accountFilter,
    groupFilter,
    search,
    mailFilter,
    activeLabel,
    activeMailbox,
    selectedId,
    setSelectedId,
    setContacts,
    setNotice,
    isMessageActionActive: (messageId) => messageActionsRef.current.isActive(messageId),
  });
  const openNotificationMessage = useCallback((messageId: string, accountEmail?: string) => {
    const account = realAccountsRef.current.find((item) => item.email === accountEmail);
    selectNavigationScope('inbox', account?.id ?? 'all', null);
    setMailFilter('all');
    setSelectedId(messageId);
    setSidebarOpen(false);
  }, [selectNavigationScope, setSidebarOpen]);
  useNewMailNotifications(preferences.notificationKinds.unread, openNotificationMessage);

  const load = useCallback(async () => {
    const results = await Promise.allSettled([
      api<{ accounts: Account[] }>('/api/accounts'),
      api<{ tokens: DeveloperToken[] }>('/api/developer-tokens'),
      api<MessageStats>('/api/message-stats'),
      api<{ drafts: Draft[] }>('/api/drafts'),
      api<{ labels: string[] }>('/api/labels'),
      api<{ contacts: Contact[] }>('/api/contacts'),
    ] as const);
    const [accountResult, tokenResult, statsResult, draftResult, labelResult, contactResult] = results;
    if (accountResult.status === 'rejected') {
      setNotice({ kind: 'error', text: accountResult.reason instanceof Error ? accountResult.reason.message : '邮箱账户加载失败' });
      return;
    }
    setRealAccounts(accountResult.value.accounts);
    if (tokenResult.status === 'fulfilled') setTokens(tokenResult.value.tokens);
    if (statsResult.status === 'fulfilled') setMessageStats(statsResult.value);
    if (draftResult.status === 'fulfilled') setDrafts(draftResult.value.drafts);
    if (labelResult.status === 'fulfilled') setLabels(labelResult.value.labels);
    if (contactResult.status === 'fulfilled') setContacts(contactResult.value.contacts);
    const optionalFailure = results.slice(1).find((result) => result.status === 'rejected');
    if (optionalFailure?.status === 'rejected') {
      setNotice({ kind: 'error', text: optionalFailure.reason instanceof Error ? optionalFailure.reason.message : '部分邮箱数据加载失败，已保留现有内容' });
    }
  }, [setMessageStats]);

  useEffect(() => { void load(); }, [load]);
  // Desktop callbacks are registered once and only use React setters/ref-backed state.
  useEffect(() => subscribeDesktopCompose(() => openCompose()), []);
  useEffect(() => subscribeDesktopAccountSelection((accountId) => {
    if (!realAccountsRef.current.some((account) => account.id === accountId)) return;
    selectScope('inbox', accountId);
  // Account selection reads the current account snapshot through realAccountsRef.
  }), []);
  useLayoutEffect(() => { setTheme(preferences.theme, preferences.customTheme); }, [preferences.customTheme, preferences.theme, setTheme]);
  useEffect(() => {
    void updateDesktopTrayMenu(accounts, preferences.theme, preferences.customTheme)
      .catch((error) => desktopLog('warn', 'tray.update_failed', describeDesktopLogValue(error)));
  }, [accounts, preferences.customTheme, preferences.theme]);
  useEffect(() => { realAccountsRef.current = realAccounts; }, [realAccounts]);
  useEffect(() => {
    if (folderDiscoveryStarted.current || accounts.length === 0 || accounts.some((account) => account.mailboxes.length > 0)) return;
    folderDiscoveryStarted.current = true;
    void api('/api/sync', { method: 'POST' }).then(load).catch((error) => {
      folderDiscoveryStarted.current = false;
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮箱文件夹初始化失败' });
    });
  }, [accounts, load]);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 4200);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const groups = useMemo(() => Array.from(new Set(accounts.map((account) => account.group))), [accounts]);
  const workspaceFolders = useMemo(() => buildWorkspaceFolders(accounts, groups), [accounts, groups]);
  const selectedIndex = selected ? messages.findIndex((message) => message.id === selected.id) : -1;
  const activeAccount = accountFilter === 'all' ? undefined : accounts.find((account) => account.id === accountFilter);
  const visibleMessages = mailFilter === 'unread' ? messages.filter((message) => message.unread) : messages;

  useEffect(() => {
    if (!['sent', 'archive', 'drafts', 'trash', 'junk'].includes(view) || accounts.length === 0) return;
    let cancelled = false;
    void api(`/api/mailboxes/${view}/sync`, { method: 'POST' })
      .catch((error) => { if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '文件夹同步失败' }); });
    return () => { cancelled = true; };
  }, [accounts.length, view]);

  useEffect(() => {
    if (view !== 'folder' || !activeMailbox) return;
    let cancelled = false;
    void Promise.all(activeMailbox.targets.map((target) => api(`/api/accounts/${target.accountId}/mailboxes/sync`, { method: 'POST', body: JSON.stringify({ mailbox: target.path }) })))
      .catch((error) => { if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '文件夹同步失败' }); });
    return () => { cancelled = true; };
  }, [view, activeMailbox]);

  async function syncAll() {
    if (realAccounts.length === 0) { setAddOpen(true); return; }
    try {
      const role = ['sent', 'archive', 'drafts', 'trash', 'junk'].includes(view)
        ? view
        : view === 'inbox' || view === 'starred' || view === 'snoozed' || view === 'contacts' ? 'inbox' : null;
      if (view === 'folder' && activeMailbox) await Promise.all(activeMailbox.targets.map((target) => api(`/api/accounts/${target.accountId}/mailboxes/sync`, { method: 'POST', body: JSON.stringify({ mailbox: target.path }) })));
      else if (role) await api(role === 'inbox' ? '/api/sync' : `/api/mailboxes/${role}/sync`, { method: 'POST' });
      setNotice({ kind: 'success', text: '同步任务已加入后台队列' });
    }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '同步失败' }); }
  }

  async function openNotifications() {
    try { const result = await api<{ notifications: MailNotification[] }>('/api/notifications'); setNotifications(result.notifications.filter((item) => preferences.notificationKinds[item.kind])); setNotificationsOpen(true); }
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

  async function setMessageUnread(message: Message, unread: boolean) {
    if (message.unread === unread || messageActionsRef.current.isActive(message.id)) return;
    await messageActionsRef.current.run(message.id, async () => {
      setMessageActionBusy(true);
      setRealMessages((current) => applyOptimisticMessageMutation(current, message.id, { unread }));
      if (mailFilter === 'unread') setMessageTotal((current) => Math.max(0, current + (unread ? 1 : -1)));
      if (message.mailboxRole === 'inbox') adjustMessageStats(message.accountId, 0, unread ? 1 : -1);
      try {
        await api(`/api/messages/${message.id}`, { method: 'PATCH', body: JSON.stringify({ unread }) });
        setNotice({ kind: 'success', text: unread ? '邮件已标记为未读' : '邮件已标记为已读' });
      } catch (error) {
        setRealMessages((current) => rollbackOptimisticMessageMutation(current, message.id, { unread }, { unread: message.unread }));
        if (mailFilter === 'unread') setMessageTotal((current) => Math.max(0, current + (unread ? -1 : 1)));
        if (message.mailboxRole === 'inbox') adjustMessageStats(message.accountId, 0, unread ? -1 : 1);
        setNotice({ kind: 'error', text: error instanceof Error ? error.message : '已读状态更新失败' });
      } finally { setMessageRevision((value) => value + 1); setMessageActionBusy(false); }
    });
  }

  async function markSelectedUnread() { if (selected) await setMessageUnread(selected, true); }

  async function toggleSelectedFlag(target = selected) {
    if (!target || messageActionsRef.current.isActive(target.id)) return;
    const flagged = !target.flagged;
    await messageActionsRef.current.run(target.id, async () => {
      setMessageActionBusy(true);
      setRealMessages((current) => applyOptimisticMessageMutation(current, target.id, { flagged }));
      try {
        await api(`/api/messages/${target.id}`, { method: 'PATCH', body: JSON.stringify({ flagged }) });
        if (view === 'starred' && !flagged) setMessageRevision((value) => value + 1);
      } catch (error) {
        setRealMessages((current) => rollbackOptimisticMessageMutation(current, target.id, { flagged }, { flagged: target.flagged }));
        setNotice({ kind: 'error', text: error instanceof Error ? error.message : '星标更新失败' });
      } finally { setMessageRevision((value) => value + 1); setMessageActionBusy(false); }
    });
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

  async function selectMessage(id: string) {
    if (composeMode) await composePaneRef.current?.close();
    setSelectedId(id);
    const message = messages.find((item) => item.id === id);
    if (!message?.unread || !preferences.markReadOnOpen) return;

    void setMessageUnread(message, false);
  }

  async function moveSelected(destination: 'archive' | 'trash', target = selected) {
    if (!target || messageActionBusy || !messageActionsRef.current.begin(target.id)) return;
    const message = target;
    const index = messages.findIndex((item) => item.id === message.id);
    const nextId = messages[index + 1]?.id ?? messages[index - 1]?.id ?? null;
    const unreadDelta = message.unread ? -1 : 0;
    setMessageActionBusy(true);
    setRealMessages((current) => current.filter((item) => item.id !== message.id));
    setMessageTotal((current) => Math.max(0, current - 1));
    if (message.mailboxRole === 'inbox') adjustMessageStats(message.accountId, -1, unreadDelta);
    setSelectedId(nextId);
    setPendingMove({ message, index, nextId, destination, unreadDelta });
  }

  function restorePendingMove(move: PendingMove) {
    const { message, index, unreadDelta } = move;
    setRealMessages((current) => {
      if (current.some((item) => item.id === message.id)) return current;
      const restored = [...current]; restored.splice(Math.min(index, restored.length), 0, message); return restored;
    });
    setMessageTotal((current) => current + 1);
    if (message.mailboxRole === 'inbox') adjustMessageStats(message.accountId, 1, -unreadDelta);
    setSelectedId(message.id);
    setMessageActionBusy(false);
  }

  useEffect(() => {
    if (!pendingMove) return;
    const timer = window.setTimeout(() => {
      const move = pendingMove;
      setPendingMove(null);
      void api(`/api/messages/${move.message.id}/move`, { method: 'POST', body: JSON.stringify({ destination: move.destination }) }).then(() => {
        setNotice({ kind: 'success', text: move.destination === 'archive' ? '邮件已归档' : '邮件已移至垃圾箱' });
      }).catch((error) => {
        restorePendingMove(move);
        setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮件移动失败' });
      }).finally(() => {
        messageActionsRef.current.end(move.message.id);
        setMessageRevision((value) => value + 1); setMessageActionBusy(false);
      });
    }, 4500);
    return () => window.clearTimeout(timer);
  }, [pendingMove]);

  function undoPendingMove() {
    if (!pendingMove) return;
    const move = pendingMove;
    setPendingMove(null);
    restorePendingMove(move);
    messageActionsRef.current.end(move.message.id);
    setNotice({ kind: 'success', text: '已撤销邮件移动' });
  }

  function selectScope(nextView: AppView, nextAccount = 'all', nextGroup: string | null = null) {
    selectNavigationScope(nextView, nextAccount, nextGroup); setSelectedId(null);
  }

  function selectMailbox(folder: WorkspaceFolder) {
    selectNavigationMailbox(folder); setSelectedId(null);
  }

  function selectLabel(label: string) { selectNavigationLabel(label); setSelectedId(null); }

  function focusSearch() {
    const input = document.querySelector<HTMLInputElement>('.search-box input') ?? searchInputRef.current;
    input?.focus(); input?.select();
  }

  function openCompose(accountId?: string, initialTo: string[] = []) {
    if (initialTo.length > 0) setSearch('');
    setComposeAccountId(accountId); setComposeInitialTo(initialTo); setActiveDraft(undefined); setComposeMode('new'); setView('inbox'); setSidebarOpen(false);
  }

  async function syncAccount(accountId: string) {
    try { await api(`/api/accounts/${accountId}/sync`, { method: 'POST' }); setNotice({ kind: 'success', text: '邮箱已加入后端同步队列' }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮箱同步失败' }); }
  }

  async function syncWorkspace(group: string) {
    try { await Promise.all(accounts.filter((account) => account.group === group).map((account) => api(`/api/accounts/${account.id}/sync`, { method: 'POST' }))); setNotice({ kind: 'success', text: `${group} 已加入后端同步队列` }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '工作空间同步失败' }); }
  }

  async function syncFolder(folder: WorkspaceFolder) {
    try { await Promise.all(folder.targets.map((target) => api(`/api/accounts/${target.accountId}/mailboxes/sync`, { method: 'POST', body: JSON.stringify({ mailbox: target.path }) }))); setNotice({ kind: 'success', text: `${folder.name} 已加入后端同步队列` }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '文件夹同步失败' }); }
  }

  async function openMessageContext(message: Message, point: { x: number; y: number }) {
    if (composeMode) await composePaneRef.current?.close();
    setSelectedId(message.id); setContextTarget({ kind: 'message', messageId: message.id, ...point });
  }

  useEffect(() => {
    const onShortcut = (event: KeyboardEvent) => {
      if (event.defaultPrevented || addOpen || tokenOpen || settingsTab || notificationsOpen || labelOpen || snoozeOpen || workspaceOpen !== undefined) return;
      const definition = shortcutDefinitions.find(({ id }) => shortcutMatches(event, shortcutBindings[id]));
      if (!definition) return;
      if (definition.scope === 'mail' && (view === 'contacts' || view === 'tokens')) return;
      if (isEditableShortcutTarget(event.target) && definition.id !== 'focusSearch' && definition.id !== 'openShortcutSettings') return;
      event.preventDefault(); setContextTarget(null);
      switch (definition.id) {
        case 'focusSearch': focusSearch(); break;
        case 'compose': openCompose(activeAccount?.id); break;
        case 'sync': void syncAll(); break;
        case 'nextMessage': if (selectedIndex >= 0 && selectedIndex < messages.length - 1) void selectMessage(messages[selectedIndex + 1].id); break;
        case 'previousMessage': if (selectedIndex > 0) void selectMessage(messages[selectedIndex - 1].id); break;
        case 'reply': if (selected) { setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('reply'); } break;
        case 'forward': if (selected) { setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('forward'); } break;
        case 'toggleStar': void toggleSelectedFlag(); break;
        case 'markUnread': void markSelectedUnread(); break;
        case 'archive': void moveSelected('archive'); break;
        case 'delete': void moveSelected('trash'); break;
        case 'openShortcutSettings': setSettingsTab('shortcuts'); break;
      }
    };
    window.addEventListener('keydown', onShortcut);
    return () => window.removeEventListener('keydown', onShortcut);
  });

  const scopeTitle = activeMailbox?.name ?? (activeLabel ? `标签 · ${activeLabel}` : view === 'starred' ? '星标邮件' : view === 'sent' ? '已发送' : view === 'snoozed' ? '稍后处理' : view === 'archive' ? '归档' : view === 'trash' ? '已删除邮件' : view === 'junk' ? '垃圾邮件' : '统一收件箱');

  return <div className={`app-shell ${sidebarCollapsed ? 'sidebar-collapsed' : ''}`}>
    {notice && <div className={`toast toast-${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'} aria-live={notice.kind === 'error' ? 'assertive' : 'polite'}>{notice.kind === 'success' ? <Check size={18} /> : <WarningCircle size={18} />}<span>{notice.text}</span></div>}
    {pendingMove && <div className="toast toast-success toast-action" role="status" aria-live="polite"><span>{pendingMove.destination === 'archive' ? '邮件即将归档' : '邮件即将移至垃圾箱'}</span><button type="button" onClick={undoPendingMove}><ArrowCounterClockwise size={15} />撤销</button></div>}

    <AppAccountRail user={user} accounts={accounts} accountFilter={accountFilter} onSelect={(accountId) => selectScope('inbox', accountId)} onAdd={() => setAddOpen(true)} onSettings={() => setSettingsTab('general')} onSwitchAccount={() => void logout()} onContextMenu={(event, accountId) => { event.preventDefault(); setContextTarget(accountId ? { kind: 'account', accountId, x: event.clientX, y: event.clientY } : { kind: 'background', x: event.clientX, y: event.clientY }); }} />
    <AppSidebar user={user} accounts={accounts} groups={groups} workspaceFolders={workspaceFolders} labels={labels} messageStats={messageStats} draftsCount={drafts.length} contactsCount={contacts.length} view={view} accountFilter={accountFilter} groupFilter={groupFilter} activeLabel={activeLabel} activeMailbox={activeMailbox} expandedWorkspaces={expandedWorkspaces} sidebarOpen={sidebarOpen}
      onClose={() => setSidebarOpen(false)} onCompose={openCompose} onAddAccount={() => { setAddOpen(true); setSidebarOpen(false); }} onSettings={() => { setSettingsTab('general'); setSidebarOpen(false); }} onSelectScope={selectScope} onSelectMailbox={selectMailbox} onSelectLabel={selectLabel} onEditWorkspace={setWorkspaceOpen}
      onToggleWorkspace={(group) => setExpandedWorkspaces((current) => { const next = new Set(current); if (next.has(group)) next.delete(group); else next.add(group); return next; })} onContextTarget={setContextTarget} onLogout={() => void logout()} />

    <main className="workspace">
      <AppTopbar sidebarCollapsed={sidebarCollapsed} sidebarOpen={sidebarOpen} search={search} searchPlaceholder={view === 'contacts' ? '搜索联系人姓名或邮箱' : '搜索当前范围内的邮件'} searchShortcut={shortcutLabel(shortcutBindings.focusSearch)} searchInputRef={searchInputRef} onToggleSidebar={() => setSidebarCollapsed((current) => !current)} onOpenMobileSidebar={() => setSidebarOpen(true)} onSearchChange={setSearch} onNotifications={() => void openNotifications()} />

      <WorkspaceErrorBoundary label={view === 'tokens' ? '外部接入' : view === 'contacts' ? '联系人' : composeMode ? '写信编辑器' : '邮件工作区'} resetKey={`${view}:${selected?.id ?? ''}:${composeMode ?? ''}`}>
      {view === 'contacts' ? <Suspense fallback={<FeatureFallback label="联系人" />}><ContactsWorkspace contacts={contacts} search={search} onCompose={(contact) => openCompose(activeAccount?.id, [contact.address])} /></Suspense> : view === 'tokens' ? <Suspense fallback={<FeatureFallback label="外部接入" />}><TokenWorkspace accounts={realAccounts} tokens={tokens} onCreateApi={() => setTokenOpen('api')} onCreateMcp={() => setTokenOpen('mcp')} onReload={load} setNotice={setNotice} /></Suspense> :
        <div className={`mail-layout ${selectedId || composeMode ? 'mobile-reader-open' : ''}`}>
          {view === 'drafts' ? <Suspense fallback={<FeatureFallback label="草稿" />}><DraftWorkspace drafts={drafts} remoteDrafts={messages} accounts={accounts} selectedRemoteId={selected?.id} onOpen={(draft) => { setActiveDraft(draft); setComposeMode('new'); }} onOpenRemote={(draft) => void selectMessage(draft.id)} onDelete={async (id) => { try { await api(`/api/drafts/${id}`, { method: 'DELETE' }); setDrafts((current) => current.filter((draft) => draft.id !== id)); if (activeDraft?.id === id) { setActiveDraft(undefined); setComposeMode(null); } setNotice({ kind: 'success', text: '草稿已删除' }); } catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '草稿删除失败' }); } }} onCreate={() => { setActiveDraft(undefined); setComposeMode('new'); }} /></Suspense> : <MessagePane title={groupFilter ?? (accountFilter === 'all' ? scopeTitle : activeAccount?.displayName ?? '')} messageTotal={messageTotal} account={activeAccount} filter={mailFilter} messages={visibleMessages} accounts={accounts} selectedId={selected?.id} ready={ready} loading={messagesLoading} hasMore={messagesHasMore} onFilterChange={setMailFilter} onManageLabels={() => setLabelOpen(true)} onSelect={selectMessage} onContextMenu={(message, point) => void openMessageContext(message, point)} onBackgroundContextMenu={(point) => setContextTarget({ kind: 'background', ...point })} onLoadMore={() => void loadMoreMessages()} onAddAccount={() => setAddOpen(true)} />}
          {composeMode ? <Suspense fallback={<FeatureFallback label="写信编辑器" />}><ComposePane ref={composePaneRef} key={`${composeMode}-${(activeDraft?.id ?? selected?.id ?? composeAccountId ?? composeInitialTo.join(',')) || 'new'}`} accounts={realAccounts} contacts={contacts} mode={composeMode} initialAccountId={composeAccountId} initialTo={composeInitialTo} original={composeMode === 'new' ? undefined : selected} draft={activeDraft} onClose={() => { setComposeMode(null); setComposeAccountId(undefined); setComposeInitialTo([]); setActiveDraft(undefined); }} onDraftSaved={(saved) => { setDrafts((current) => [saved, ...current.filter((item) => item.id !== saved.id)]); }} onSent={async () => { setComposeMode(null); setComposeAccountId(undefined); setComposeInitialTo([]); setActiveDraft(undefined); await load(); setNotice({ kind: 'success', text: '邮件已发送' }); }} /></Suspense> : view === 'drafts' && !selected ? <Suspense fallback={<FeatureFallback label="草稿" />}><DraftWelcome onCreate={() => openCompose(activeAccount?.id)} /></Suspense> : <MessageReader message={selected} account={selected ? accounts.find((item) => item.id === selected.accountId) : undefined} defaultBodyView={preferences.defaultMessageView} onReply={() => { setComposeAccountId(undefined); setComposeInitialTo([]); setActiveDraft(undefined); setComposeMode('reply'); }} onForward={() => { setComposeAccountId(undefined); setComposeInitialTo([]); setActiveDraft(undefined); setComposeMode('forward'); }} onCloseMobile={() => setSelectedId(null)} onContextMenu={(message, point) => void openMessageContext(message, point)}
            onToggleFlag={() => void toggleSelectedFlag()}
            onSnooze={() => setSnoozeOpen(true)} onManageLabels={() => setLabelOpen(true)} onMarkUnread={() => void markSelectedUnread()}
            onArchive={() => void moveSelected('archive')} onDelete={() => void moveSelected('trash')} actionBusy={messageActionBusy}
            onPrevious={() => { if (selectedIndex > 0) selectMessage(messages[selectedIndex - 1].id); }}
            onNext={() => { if (selectedIndex >= 0 && selectedIndex < messages.length - 1) selectMessage(messages[selectedIndex + 1].id); }}
            hasPrevious={selectedIndex > 0} hasNext={selectedIndex >= 0 && selectedIndex < messages.length - 1} />}
        </div>}
      </WorkspaceErrorBoundary>
    </main>

    {addOpen && <Suspense fallback={<FeatureFallback label="邮箱接入" />}><AddAccountModal accounts={accounts} onClose={() => setAddOpen(false)} onAdded={async (result) => { setAddOpen(false); await load(); setMessageRevision((value) => value + 1); setNotice(result?.warning ? { kind: 'error', text: `授权已保存，连接验证失败：${result.warning}` } : { kind: 'success', text: '邮箱已接入，正在准备统一收件箱' }); }} /></Suspense>}
    {tokenOpen === 'api' && <Suspense fallback={<FeatureFallback label="访问令牌" />}><CreateApiTokenModal accounts={realAccounts} onClose={() => setTokenOpen(null)} onCreated={load} /></Suspense>}
    {tokenOpen === 'mcp' && <Suspense fallback={<FeatureFallback label="MCP 令牌" />}><CreateMcpTokenModal onClose={() => setTokenOpen(null)} onCreated={load} /></Suspense>}
    {settingsTab && <FeatureErrorBoundary label="设置" resetKey={settingsTab} onClose={() => setSettingsTab(null)}><Suspense fallback={<FeatureFallback label="设置" />}><SettingsModal initialTab={settingsTab} accounts={realAccounts} preferences={preferences} bindings={shortcutBindings} onPreferencesChange={savePreferences} onBindingsChange={saveShortcutBindings} onAddAccount={() => { setSettingsTab(null); setAddOpen(true); }} onClose={() => setSettingsTab(null)} onReload={load} setNotice={setNotice} /></Suspense></FeatureErrorBoundary>}
    {notificationsOpen && <Suspense fallback={<FeatureFallback label="通知" />}><NotificationsModal notifications={notifications} accounts={accounts} onClose={() => setNotificationsOpen(false)} onOpenMessage={(notification) => { setNotificationsOpen(false); if (notification.messageId) openNotificationMessage(notification.messageId, accounts.find((item) => item.id === notification.accountId)?.email); }} /></Suspense>}
    {labelOpen && selected && <Suspense fallback={<FeatureFallback label="标签" />}><LabelModal message={selected} knownLabels={labels} onClose={() => setLabelOpen(false)} onSave={(next) => { setLabelOpen(false); void updateSelectedLocal({ labels: next }, '邮件标签已更新'); }} /></Suspense>}
    {snoozeOpen && selected && <Suspense fallback={<FeatureFallback label="稍后处理" />}><SnoozeModal onClose={() => setSnoozeOpen(false)} onSave={(until) => { setSnoozeOpen(false); void updateSelectedLocal({ snoozedUntil: until }, until ? '邮件已移到稍后处理' : '邮件已返回收件箱'); }} /></Suspense>}
    {workspaceOpen !== undefined && <Suspense fallback={<FeatureFallback label="工作空间" />}><WorkspaceModal accounts={accounts} workspace={workspaceOpen ?? undefined} onClose={() => setWorkspaceOpen(undefined)} onSaved={async () => { setWorkspaceOpen(undefined); await load(); setNotice({ kind: 'success', text: '工作空间已更新' }); }} /></Suspense>}
    {contextTarget && <Suspense fallback={null}><AppContextMenu target={contextTarget} bindings={shortcutBindings} messages={messages} accounts={accounts} activeAccountId={activeAccount?.id} onClose={() => setContextTarget(null)} actions={{
      openMessage: (id) => void selectMessage(id),
      reply: () => { setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('reply'); },
      forward: () => { setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('forward'); },
      toggleStar: (message) => void toggleSelectedFlag(message), setUnread: (message, unread) => void setMessageUnread(message, unread),
      snooze: () => setSnoozeOpen(true), labels: () => setLabelOpen(true), archive: (message) => void moveSelected('archive', message), delete: (message) => void moveSelected('trash', message),
      openAccount: (id) => selectScope('inbox', id), compose: openCompose, syncAccount: (id) => void syncAccount(id), accountSettings: () => setSettingsTab('accounts'),
      openWorkspace: (group) => selectScope('inbox', 'all', group), syncWorkspace: (group) => void syncWorkspace(group), editWorkspace: setWorkspaceOpen,
      openFolder: selectMailbox, syncFolder: (folder) => void syncFolder(folder), syncCurrent: () => void syncAll(), shortcutSettings: () => setSettingsTab('shortcuts'),
    }} /></Suspense>}
    {preferencesSyncIssue && <Suspense fallback={null}><PreferencesSyncErrorDialog message={preferencesSyncIssue.message} onClose={dismissPreferencesSyncIssue} /></Suspense>}
  </div>;
}

export default App;
