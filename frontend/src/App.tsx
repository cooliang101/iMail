import { Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'preact/compat';
import { ArrowCounterClockwise, Check, WarningCircle } from './components/icons';
import { accountsFromResponse, api, contactsFromResponse, desktopLog, describeDesktopLogValue, draftsFromResponse, notificationsFromResponse, outboxFromResponse, responseStringArray, tokensFromResponse, workItemsFromResponse } from './services';
import { buildWorkspaceFolders } from './app/selectors';
import type { Account, Contact, DeveloperToken, Draft, MailWorkItemView, Message, OutboxItem, ProviderId } from './types';
import type { ApiGatewayCredential, AppView, ContextTarget, MailNotification, MailParticipant, Notice, ParticipantRole, SearchFilters, SmartFolder, WorkspaceFolder } from './app-model';
import { AppAccountRail, AppSidebar, AppTopbar, useWorkspaceNavigation } from './features/navigation';
import { buildMessageQuery, clearParticipantFilter, EMPTY_PARTICIPANT_FILTERS, MessageActionCoordinator, MessagePane, MessageReader, messageStatsFromResponse, setParticipantFilter, type MailListFilter, useMessageActions, useMessageCollection } from './features/mail';
import { AdvancedSearchPanel } from './features/search/AdvancedSearchPanel';
import { SmartFoldersNav } from './features/search/SmartFoldersNav';
import { useSmartFolders } from './features/search/useSmartFolders';
import { filtersFromQuery } from './features/search/search-model';
import { ContactsWorkspace } from './features/contacts/ContactsWorkspace';
import { DraftWelcome } from './features/compose/DraftWelcome';
import { DraftWorkspace } from './features/compose/DraftWorkspace';
import { OutboxWelcome, OutboxWorkspace } from './features/compose/OutboxWorkspace';
import { WorkQueueWorkspace } from './features/work-queue/WorkQueueWorkspace';
import { TokenWorkspace } from './features/developer/TokenWorkspace';
import type { ComposePaneHandle } from './features/compose/ComposePane';
import { isEditableShortcutTarget, shortcutDefinitions, shortcutLabel, shortcutMatches } from './features/shortcuts';
import type { SettingsTab } from './features/settings/SettingsModal';
import { useAppPreferences } from './features/settings/useAppPreferences';
import { useAuth } from './features/auth';
import { useAppTheme } from './features/appearance';
import { subscribeDesktopAccountSelection, subscribeDesktopCompose, updateDesktopTrayMenu } from './platform/desktop-events';
import { useNewMailNotifications } from './features/notifications';
import { useI18n } from './features/i18n';
import { FeatureErrorBoundary, WorkspaceErrorBoundary } from './components/ErrorBoundary';
import { AddAccountModal, AppContextMenu, ComposePane, CreateApiTokenModal, CreateMcpTokenModal, LabelModal, NotificationsModal, PreferencesSyncErrorDialog, preloadDeferredFeaturesDuringIdle, SettingsModal, SnoozeModal, WorkspaceModal } from './app/lazy-features';
import { MOBILE_MAIL_QUERY, NOTICE_VISIBLE_MS, OUTBOX_REFRESH_INTERVAL_MS } from './app/constants';
import { useOutboxActions } from './features/compose/useOutboxActions';
import { useWorkQueueActions } from './features/work-queue/useWorkQueueActions';
import { applySettledResult } from './app/settled-result';

function FeatureFallback({ label, kind = 'overlay' }: { label: string; kind?: 'workspace' | 'pane' | 'overlay' }) {
  const { t } = useI18n();
  return <div className={`feature-loading feature-loading-${kind}`} role="status">{t('正在加载{label}…', { label: t(label) })}</div>;
}

type AddAccountIntent = { initialProvider?: ProviderId; managedIcloud?: boolean; returnToSettings?: SettingsTab };

function App() {
  const { t } = useI18n();
  const { user, logout } = useAuth();
  const { setTheme } = useAppTheme();
  const [realAccounts, setRealAccounts] = useState<Account[]>([]);
  const [tokens, setTokens] = useState<DeveloperToken[]>([]);
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [outbox, setOutbox] = useState<OutboxItem[]>([]);
  const [workItems, setWorkItems] = useState<MailWorkItemView[]>([]);
  const [contacts, setContacts] = useState<Contact[]>([]);
  const [labels, setLabels] = useState<string[]>([]);
  const [notice, setNotice] = useState<Notice>(null);
  const { preferences, shortcutBindings, preferencesSyncIssue, dismissPreferencesSyncIssue, savePreferences, saveShortcutBindings } = useAppPreferences(user.id, setNotice);
  const { searchFilters, setSearchFilters, smartFolderId, setSmartFolderId, view, setView, accountFilter, setAccountFilter, groupFilter, setGroupFilter, search, setSearch, participantFilters, setParticipantFilters, activeLabel, activeMailbox, sidebarOpen, setSidebarOpen, sidebarCollapsed, setSidebarCollapsed, selectScope: selectNavigationScope, selectMailbox: selectNavigationMailbox, selectLabel: selectNavigationLabel } = useWorkspaceNavigation(preferences.startupView);
  const smartFolders = useSmartFolders(user.id);
  const [searchEditor, setSearchEditor] = useState<{ filters: SearchFilters; folder?: SmartFolder } | null>(null);
  const [mailFilter, setMailFilter] = useState<MailListFilter>('all');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [addAccountIntent, setAddAccountIntent] = useState<AddAccountIntent | null>(null);
  const [conversationComposeSource, setConversationComposeSource] = useState<Message>();
  const [composeMode, setComposeMode] = useState<'new' | 'reply' | 'replyAll' | 'forward' | null>(null);
  const [tokenOpen, setTokenOpen] = useState<'api' | 'mcp' | null>(null);
  const [latestApiCredential, setLatestApiCredential] = useState<ApiGatewayCredential>();
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
  const realAccountsRef = useRef<Account[]>([]);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const folderDiscoveryStarted = useRef(false);
  const composePaneRef = useRef<ComposePaneHandle | null>(null);
  const messageActionsRef = useRef(new MessageActionCoordinator());

  useEffect(() => { setLatestApiCredential(undefined); }, [user.id]);

  const accounts = realAccounts;
  const { messages, setMessages: setRealMessages, messageTotal, setMessageTotal, hasMore: messagesHasMore, loading: messagesLoading, ready, setRevision: setMessageRevision, stats: messageStats, setStats: setMessageStats, selected, loadMore: loadMoreMessages } = useMessageCollection({
    accounts,
    view,
    accountFilter,
    groupFilter,
    search,
    participantFilters,
    searchFilters,
    mailFilter,
    activeLabel,
    activeMailbox,
    selectedId,
    setSelectedId,
    setContacts,
    setNotice,
    isMessageActionActive: (messageId) => messageActionsRef.current.isActive(messageId),
  });
  useEffect(() => {
    if (view !== 'workQueue') return;
    setRealMessages(workItems.map(({ message }) => message));
    setMessageTotal(workItems.length);
    setSelectedId((current) => current && workItems.some(({ item }) => item.messageId === current) ? current : null);
  }, [setMessageTotal, setRealMessages, view, workItems]);
  const openNotificationMessage = useCallback((messageId: string, accountEmail?: string) => {
    const account = realAccountsRef.current.find((item) => item.email === accountEmail);
    selectNavigationScope('inbox', account?.id ?? 'all', null);
    setMailFilter('all');
    setSearch('');
    setParticipantFilters(EMPTY_PARTICIPANT_FILTERS);
    setSelectedId(messageId);
    setSidebarOpen(false);
  }, [selectNavigationScope, setParticipantFilters, setSearch, setSidebarOpen]);
  useNewMailNotifications(preferences.notificationKinds.unread, openNotificationMessage);

  const load = useCallback(async () => {
    const results = await Promise.allSettled([
      api<unknown>('/api/accounts'),
      api<unknown>('/api/developer-tokens'),
      api<unknown>('/api/message-stats'),
      api<unknown>('/api/drafts'),
      api<unknown>('/api/labels'),
      api<unknown>('/api/contacts'),
      api<unknown>('/api/outbox'),
      api<unknown>('/api/mail-work-items'),
    ] as const);
    const [accountResult, tokenResult, statsResult, draftResult, labelResult, contactResult, outboxResult, workItemsResult] = results;
    if (accountResult.status === 'rejected') {
      setNotice({ kind: 'error', text: accountResult.reason instanceof Error ? accountResult.reason.message : '邮箱账户加载失败' });
      return;
    }
    try {
      setRealAccounts(accountsFromResponse(accountResult.value));
    } catch (error) {
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮箱账户数据格式不正确' });
      return;
    }
    const optionalFailures = [
      applySettledResult(tokenResult, tokensFromResponse, setTokens),
      applySettledResult(statsResult, messageStatsFromResponse, setMessageStats),
      applySettledResult(draftResult, draftsFromResponse, setDrafts),
      applySettledResult(labelResult, (value) => responseStringArray(value, 'labels'), setLabels),
      applySettledResult(contactResult, contactsFromResponse, setContacts),
      applySettledResult(outboxResult, outboxFromResponse, setOutbox),
      applySettledResult(workItemsResult, workItemsFromResponse, setWorkItems),
    ].filter((failure) => failure !== undefined);
    if (optionalFailures.length > 0) {
      const failure = optionalFailures[0];
      setNotice({ kind: 'error', text: failure instanceof Error ? failure.message : '部分邮箱数据加载失败，已保留现有内容' });
    }
  }, [setMessageStats]);

  const loadOutbox = useCallback(async () => {
    const result = await api<unknown>('/api/outbox');
    setOutbox(outboxFromResponse(result));
  }, []);

  const { cancelOutboxItem, retryOutboxItem, resolveOutboxItem } = useOutboxActions({ reloadAll: load, reloadOutbox: loadOutbox, setNotice });
  const { setWorkItem, completeWorkItem } = useWorkQueueActions({ workItems, selectedId, setSelectedId, setWorkItems, setNotice });

  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    let active = true;
    const refresh = () => void api<unknown>('/api/outbox')
      .then((result) => { if (active) setOutbox(outboxFromResponse(result)); })
      .catch(() => undefined);
    const timer = window.setInterval(refresh, OUTBOX_REFRESH_INTERVAL_MS);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  useEffect(() => {
    if (!ready) return;
    return preloadDeferredFeaturesDuringIdle();
  }, [ready]);
  // Desktop callbacks are registered once and only use state setters/ref-backed snapshots.
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
    const timer = window.setTimeout(() => setNotice(null), NOTICE_VISIBLE_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const groups = useMemo(() => Array.from(new Set(accounts.map((account) => account.group))), [accounts]);
  const workspaceFolders = useMemo(() => buildWorkspaceFolders(accounts, groups), [accounts, groups]);
  const selectedIndex = selected ? messages.findIndex((message) => message.id === selected.id) : -1;
  const activeAccount = accountFilter === 'all' ? undefined : accounts.find((account) => account.id === accountFilter);
  const visibleMessages = mailFilter === 'unread' ? messages.filter((message) => message.unread) : messages;
  const { messageActionBusy, pendingMove, setMessageUnread, markSelectedUnread, toggleSelectedFlag, moveSelected, undoPendingMove } = useMessageActions({ accounts, messages, selected, view, mailFilter, coordinator: messageActionsRef.current, setMessages: setRealMessages, setMessageTotal, setMessageStats, setMessageRevision, setSelectedId, setNotice });

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
    if (realAccounts.length === 0) { setAddAccountIntent({}); return; }
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
    try { const result = await api<unknown>('/api/notifications'); setNotifications(notificationsFromResponse(result).filter((item) => preferences.notificationKinds[item.kind])); setNotificationsOpen(true); }
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

  async function selectMessage(id: string) {
    if (composeMode && !await composePaneRef.current?.close()) return;
    setSelectedId(id);
    const message = messages.find((item) => item.id === id);
    if (!message?.unread || !preferences.markReadOnOpen) return;

    void setMessageUnread(message, false);
  }

  function openAdvancedSearch() {
    const filters = filtersFromQuery(buildMessageQuery({ accountFilter, groupFilter, search, view, mailFilter, activeLabel, activeMailbox, participantFilters, searchFilters }));
    setSearchEditor({ filters, folder: smartFolderId ? smartFolders.folders.find((folder) => folder.id === smartFolderId) : undefined });
  }

  async function applySearch(filters: SearchFilters, folderId?: string) {
    if (composePaneRef.current && !await composePaneRef.current.close()) return false;
    selectNavigationScope('search');
    setSearchFilters(filters); setSearch(filters.q ?? ''); setSmartFolderId(folderId ?? null);
    setMailFilter('all'); setParticipantFilters(EMPTY_PARTICIPANT_FILTERS); setSelectedId(null);
    return true;
  }

  async function deleteSmartFolder(id: string) {
    await smartFolders.remove(id);
    if (smartFolderId === id) setSmartFolderId(null);
  }

  async function selectScope(nextView: AppView, nextAccount = 'all', nextGroup: string | null = null) {
    if (composePaneRef.current && !await composePaneRef.current.close()) return;
    selectNavigationScope(nextView, nextAccount, nextGroup); setSelectedId(null);
  }

  async function selectMailbox(folder: WorkspaceFolder) {
    if (composePaneRef.current && !await composePaneRef.current.close()) return;
    selectNavigationMailbox(folder); setSelectedId(null);
  }

  async function selectLabel(label: string) {
    if (composePaneRef.current && !await composePaneRef.current.close()) return;
    selectNavigationLabel(label); setSelectedId(null);
  }

  function filterParticipant(role: ParticipantRole, participant: MailParticipant) {
    setParticipantFilters((current) => setParticipantFilter(current, role, participant));
    if (window.matchMedia(MOBILE_MAIL_QUERY).matches) setSelectedId(null);
  }

  function removeParticipantFilter(role: ParticipantRole) {
    setParticipantFilters((current) => clearParticipantFilter(current, role));
  }

  function clearParticipantFilters() { setParticipantFilters(EMPTY_PARTICIPANT_FILTERS); }

  function keepOnlyParticipantFilters() {
    setSearch('');
    setMailFilter('all');
    if (view === 'search') { setSearchFilters({}); setSmartFolderId(null); }
  }

  function focusSearch() {
    const input = document.querySelector<HTMLInputElement>('.search-box input') ?? searchInputRef.current;
    input?.focus(); input?.select();
  }

  async function openDraft(draft?: Draft) {
    if (composePaneRef.current && !await composePaneRef.current.close()) return;
    setConversationComposeSource(undefined); setActiveDraft(draft); setComposeMode('new');
  }

  async function openCompose(accountId?: string, initialTo: string[] = []) {
    if (composePaneRef.current && !await composePaneRef.current.close()) return;
    setConversationComposeSource(undefined);
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
    if (composeMode && !await composePaneRef.current?.close()) return;
    setSelectedId(message.id); setContextTarget({ kind: 'message', messageId: message.id, ...point });
  }

  useEffect(() => {
    const onShortcut = (event: KeyboardEvent) => {
      if (event.defaultPrevented || addAccountIntent || tokenOpen || settingsTab || notificationsOpen || labelOpen || snoozeOpen || workspaceOpen !== undefined) return;
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
        case 'reply': if (!composeMode && selected?.text !== undefined) { setConversationComposeSource(selected); setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('reply'); } break;
        case 'forward': if (!composeMode && selected?.text !== undefined) { setConversationComposeSource(selected); setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('forward'); } break;
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

  const scopeTitle = activeMailbox?.name ?? (activeLabel ? t('标签 · {label}', { label: activeLabel }) : t(view === 'starred' ? '星标邮件' : view === 'sent' ? '已发送' : view === 'snoozed' ? '稍后处理' : view === 'archive' ? '归档' : view === 'trash' ? '已删除邮件' : view === 'junk' ? '垃圾邮件' : '统一收件箱'));

  return <div className={`app-shell ${sidebarCollapsed ? 'sidebar-collapsed' : ''}`}>
    {notice && <div className={`toast toast-${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'} aria-live={notice.kind === 'error' ? 'assertive' : 'polite'}>{notice.kind === 'success' ? <Check size={18} /> : <WarningCircle size={18} />}<span>{notice.text}</span></div>}
    {pendingMove && <div className="toast toast-success toast-action" role="status" aria-live="polite"><span>{t(pendingMove.destination === 'archive' ? '邮件即将归档' : '邮件即将移至垃圾箱')}</span><button type="button" onClick={undoPendingMove}><ArrowCounterClockwise size={15} />{t('撤销')}</button></div>}

    {searchEditor && <AdvancedSearchPanel initial={searchEditor.filters} folder={searchEditor.folder} accounts={accounts} labels={labels} onClose={() => setSearchEditor(null)} onApply={applySearch} onSave={smartFolders.save} onDelete={deleteSmartFolder} />}
    <AppAccountRail user={user} accounts={accounts} accountFilter={accountFilter} onSelect={(accountId) => selectScope('inbox', accountId)} onAdd={() => setAddAccountIntent({})} onSettings={() => setSettingsTab('general')} onSwitchAccount={() => void logout()} onContextMenu={(event, accountId) => { event.preventDefault(); setContextTarget(accountId ? { kind: 'account', accountId, x: event.clientX, y: event.clientY } : { kind: 'background', x: event.clientX, y: event.clientY }); }} />
    <AppSidebar smartFolders={<SmartFoldersNav folders={smartFolders.folders} activeId={view === 'search' ? smartFolderId : null} loading={smartFolders.loading} error={smartFolders.error} onRetry={() => void smartFolders.reload()} onSelect={(folder) => void applySearch(folder.filters, folder.id)} onEdit={(folder) => setSearchEditor({ filters: folder.filters, folder })} onCreate={() => setSearchEditor({ filters: {} })} />} user={user} accounts={accounts} groups={groups} workspaceFolders={workspaceFolders} labels={labels} messageStats={messageStats} draftsCount={drafts.length} outboxCount={outbox.filter((item) => ['scheduled', 'sending', 'failed', 'needsReview'].includes(item.status)).length} workQueueCount={workItems.length} contactsCount={contacts.length} view={view} accountFilter={accountFilter} groupFilter={groupFilter} activeLabel={activeLabel} activeMailbox={activeMailbox} expandedWorkspaces={expandedWorkspaces} sidebarOpen={sidebarOpen}
      onClose={() => setSidebarOpen(false)} onCompose={openCompose} onAddAccount={() => { setAddAccountIntent({}); setSidebarOpen(false); }} onSettings={() => { setSettingsTab('general'); setSidebarOpen(false); }} onSelectScope={selectScope} onSelectMailbox={selectMailbox} onSelectLabel={selectLabel} onEditWorkspace={setWorkspaceOpen}
      onToggleWorkspace={(group) => setExpandedWorkspaces((current) => { const next = new Set(current); if (next.has(group)) next.delete(group); else next.add(group); return next; })} onContextTarget={setContextTarget} onLogout={() => void logout()} />

    <main className="workspace">
      <AppTopbar onAdvancedSearch={view !== 'contacts' && view !== 'tokens' ? openAdvancedSearch : undefined} advancedActive={view === 'search'} sidebarCollapsed={sidebarCollapsed} sidebarOpen={sidebarOpen} search={search} searchPlaceholder={view === 'contacts' ? '搜索联系人姓名或邮箱' : '搜索当前范围内的邮件'} searchShortcut={shortcutLabel(shortcutBindings.focusSearch)} searchInputRef={searchInputRef} onToggleSidebar={() => setSidebarCollapsed((current) => !current)} onOpenMobileSidebar={() => setSidebarOpen(true)} onSearchChange={setSearch} onAbout={() => setSettingsTab('about')} onNotifications={() => void openNotifications()} />

      <WorkspaceErrorBoundary label={view === 'tokens' ? '外部接入' : view === 'contacts' ? '联系人' : composeMode ? '写信编辑器' : '邮件工作区'} resetKey={`${view}:${selected?.id ?? ''}:${composeMode ?? ''}`}>
      {view === 'contacts' ? <Suspense fallback={<FeatureFallback label="联系人" />}><ContactsWorkspace contacts={contacts} search={search} onCompose={(contact) => openCompose(activeAccount?.id, [contact.address])} /></Suspense> : view === 'tokens' ? <Suspense fallback={<FeatureFallback label="外部接入" />}><TokenWorkspace accounts={realAccounts} tokens={tokens} latestApiCredential={latestApiCredential} onCreateApi={() => setTokenOpen('api')} onCreateMcp={() => setTokenOpen('mcp')} onReload={load} setNotice={setNotice} /></Suspense> :
        <div className={`mail-layout ${selectedId || composeMode ? 'mobile-reader-open' : ''}`}>
          {view === 'outbox' ? <OutboxWorkspace items={outbox} accounts={accounts} onCancel={cancelOutboxItem} onRetry={retryOutboxItem} onResolve={resolveOutboxItem} /> : view === 'workQueue' ? <WorkQueueWorkspace items={workItems} accounts={accounts} selectedId={selected?.id} onSelect={(id) => void selectMessage(id)} onUpdate={setWorkItem} onComplete={completeWorkItem} onOpenDraft={(id) => void openDraft(drafts.find((draft) => draft.id === id))} /> : view === 'drafts' ? <Suspense fallback={<FeatureFallback label="草稿" />}><DraftWorkspace drafts={drafts} remoteDrafts={messages} accounts={accounts} selectedRemoteId={selected?.id} onOpen={(draft) => void openDraft(draft)} onOpenRemote={(draft) => void selectMessage(draft.id)} onDelete={async (id) => { try { await api(`/api/drafts/${id}`, { method: 'DELETE' }); setDrafts((current) => current.filter((draft) => draft.id !== id)); if (activeDraft?.id === id) { setActiveDraft(undefined); setComposeMode(null); } setNotice({ kind: 'success', text: '草稿已删除' }); } catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '草稿删除失败' }); } }} onCreate={() => void openDraft()} /></Suspense> : <MessagePane title={view === 'search' ? smartFolders.folders.find((folder) => folder.id === smartFolderId)?.name ?? '高级搜索' : groupFilter ?? (accountFilter === 'all' ? scopeTitle : activeAccount?.displayName ?? '')} messageTotal={messageTotal} account={activeAccount} filter={mailFilter} participantFilters={participantFilters} hasOtherFilters={Boolean(search.trim()) || mailFilter !== 'all' || (view === 'search' && Object.keys(searchFilters ?? {}).length > 0)} messages={visibleMessages} accounts={accounts} selectedId={selected?.id} ready={ready} loading={messagesLoading} hasMore={messagesHasMore} onFilterChange={setMailFilter} onClearParticipantFilter={removeParticipantFilter} onClearParticipantFilters={clearParticipantFilters} onKeepOnlyParticipantFilters={keepOnlyParticipantFilters} onManageLabels={() => setLabelOpen(true)} onSelect={selectMessage} onContextMenu={(message, point) => void openMessageContext(message, point)} onBackgroundContextMenu={(point) => setContextTarget({ kind: 'background', ...point })} onLoadMore={() => void loadMoreMessages()} onAddAccount={() => setAddAccountIntent({})} />}
          {composeMode ? <Suspense fallback={<FeatureFallback label="写信编辑器" />}><ComposePane ref={composePaneRef} key={`${composeMode}-${(activeDraft?.id ?? (composeMode === 'new' ? composeAccountId ?? composeInitialTo.join(',') : conversationComposeSource?.id ?? selected?.id)) || 'new'}`} accounts={realAccounts} contacts={contacts} composition={preferences.composition} mode={composeMode} initialAccountId={composeAccountId} initialTo={composeInitialTo} original={composeMode === 'new' ? undefined : conversationComposeSource ?? selected} draft={activeDraft} onClose={() => { setComposeMode(null); setConversationComposeSource(undefined); setComposeAccountId(undefined); setComposeInitialTo([]); setActiveDraft(undefined); }} onDraftSaved={(saved) => { setDrafts((current) => [saved, ...current.filter((item) => item.id !== saved.id)]); }} onScheduled={async () => { setComposeMode(null); setConversationComposeSource(undefined); setComposeAccountId(undefined); setComposeInitialTo([]); setActiveDraft(undefined); await load(); selectNavigationScope('outbox'); setSelectedId(null); setNotice({ kind: 'success', text: '邮件已加入发件箱' }); }} onSent={async () => { setComposeMode(null); setConversationComposeSource(undefined); setComposeAccountId(undefined); setComposeInitialTo([]); setActiveDraft(undefined); await load(); setNotice({ kind: 'success', text: '邮件已发送' }); }} /></Suspense> : view === 'outbox' ? <OutboxWelcome /> : view === 'drafts' && !selected ? <Suspense fallback={<FeatureFallback label="草稿" />}><DraftWelcome onCreate={() => openCompose(activeAccount?.id)} /></Suspense> : <MessageReader message={selected} accounts={accounts} onComposeConversationMessage={(message, mode) => { setConversationComposeSource(message); setActiveDraft(undefined); setComposeAccountId(undefined); setComposeInitialTo([]); setComposeMode(mode); }} account={selected ? accounts.find((item) => item.id === selected.accountId) : undefined} contacts={contacts} defaultBodyView={preferences.defaultMessageView} onReplyAll={() => { setConversationComposeSource(selected); setComposeAccountId(undefined); setComposeInitialTo([]); setActiveDraft(undefined); setComposeMode('replyAll'); }} onReply={() => { setConversationComposeSource(selected); setComposeAccountId(undefined); setComposeInitialTo([]); setActiveDraft(undefined); setComposeMode('reply'); }} onForward={() => { setConversationComposeSource(selected); setComposeAccountId(undefined); setComposeInitialTo([]); setActiveDraft(undefined); setComposeMode('forward'); }} onComposeSender={(address) => openCompose(selected?.accountId, [address])} onFilterParticipant={filterParticipant} onCloseMobile={() => setSelectedId(null)} onContextMenu={(message, point) => void openMessageContext(message, point)}
            onToggleFlag={() => void toggleSelectedFlag()}
            onSnooze={() => setSnoozeOpen(true)} onAddToWorkQueue={() => { if (selected) void setWorkItem(selected.id); }} onManageLabels={() => setLabelOpen(true)} onMarkUnread={() => void markSelectedUnread()}
            onArchive={() => void moveSelected('archive')} onDelete={() => void moveSelected('trash')} actionBusy={messageActionBusy}
            onPrevious={() => { if (selectedIndex > 0) selectMessage(messages[selectedIndex - 1].id); }}
            onNext={() => { if (selectedIndex >= 0 && selectedIndex < messages.length - 1) selectMessage(messages[selectedIndex + 1].id); }}
            hasPrevious={selectedIndex > 0} hasNext={selectedIndex >= 0 && selectedIndex < messages.length - 1} />}
        </div>}
      </WorkspaceErrorBoundary>
    </main>

    {addAccountIntent && <Suspense fallback={<FeatureFallback label="邮箱接入" />}><AddAccountModal accounts={accounts} initialProvider={addAccountIntent.initialProvider} managedIcloud={addAccountIntent.managedIcloud} onClose={() => { const returnTo = addAccountIntent.returnToSettings; setAddAccountIntent(null); if (returnTo) setSettingsTab(returnTo); }} onAdded={async (result) => { const returnTo = addAccountIntent.returnToSettings; const managedIcloud = addAccountIntent.managedIcloud; setAddAccountIntent(null); await load(); setMessageRevision((value) => value + 1); if (returnTo) setSettingsTab(returnTo); setNotice(result?.warning ? { kind: 'error', text: `授权已保存，连接验证失败：${result.warning}` } : { kind: 'success', text: managedIcloud ? 'iCloud 邮箱已托管，可继续连接 Apple 服务' : '邮箱已接入，正在准备统一收件箱' }); }} /></Suspense>}
    {tokenOpen === 'api' && <Suspense fallback={<FeatureFallback label="访问令牌" />}><CreateApiTokenModal accounts={realAccounts} onClose={() => setTokenOpen(null)} onCreated={async (credential) => { setLatestApiCredential(credential); await load(); }} /></Suspense>}
    {tokenOpen === 'mcp' && <Suspense fallback={<FeatureFallback label="MCP 令牌" />}><CreateMcpTokenModal onClose={() => setTokenOpen(null)} onCreated={load} /></Suspense>}
    {settingsTab && <FeatureErrorBoundary label="设置" resetKey={settingsTab} onClose={() => setSettingsTab(null)}><Suspense fallback={<FeatureFallback label="设置" />}><SettingsModal initialTab={settingsTab} accounts={realAccounts} preferences={preferences} bindings={shortcutBindings} onPreferencesChange={savePreferences} onBindingsChange={saveShortcutBindings} onAddAccount={(provider, returnToSettings) => { setSettingsTab(null); setAddAccountIntent({ initialProvider: provider, managedIcloud: returnToSettings === 'apple-hme', returnToSettings }); }} onClose={() => setSettingsTab(null)} onReload={load} setNotice={setNotice} /></Suspense></FeatureErrorBoundary>}
    {notificationsOpen && <Suspense fallback={<FeatureFallback label="通知" />}><NotificationsModal notifications={notifications} accounts={accounts} onClose={() => setNotificationsOpen(false)} onOpenMessage={(notification) => { setNotificationsOpen(false); if (notification.messageId) openNotificationMessage(notification.messageId, accounts.find((item) => item.id === notification.accountId)?.email); }} /></Suspense>}
    {labelOpen && selected && <Suspense fallback={<FeatureFallback label="标签" />}><LabelModal message={selected} knownLabels={labels} onClose={() => setLabelOpen(false)} onSave={(next) => { setLabelOpen(false); void updateSelectedLocal({ labels: next }, '邮件标签已更新'); }} /></Suspense>}
    {snoozeOpen && selected && <Suspense fallback={<FeatureFallback label="稍后处理" />}><SnoozeModal onClose={() => setSnoozeOpen(false)} onSave={(until) => { setSnoozeOpen(false); void updateSelectedLocal({ snoozedUntil: until }, until ? '邮件已移到稍后处理' : '邮件已返回收件箱'); }} /></Suspense>}
    {workspaceOpen !== undefined && <Suspense fallback={<FeatureFallback label="工作空间" />}><WorkspaceModal accounts={accounts} workspace={workspaceOpen ?? undefined} onClose={() => setWorkspaceOpen(undefined)} onSaved={async () => { setWorkspaceOpen(undefined); await load(); setNotice({ kind: 'success', text: '工作空间已更新' }); }} /></Suspense>}
    {contextTarget && <Suspense fallback={null}><AppContextMenu target={contextTarget} bindings={shortcutBindings} messages={messages} accounts={accounts} activeAccountId={activeAccount?.id} onClose={() => setContextTarget(null)} actions={{
      openMessage: (id) => void selectMessage(id),
      reply: () => { setConversationComposeSource(selected); setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('reply'); },
      forward: () => { setConversationComposeSource(selected); setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('forward'); },
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
