import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button } from '@fluentui/react-components';
import { Archive, ArrowClockwise, ArrowRight, Bell, CaretDown, Check, Clock, Code, FolderSimplePlus, Gear, Tray, MagnifyingGlass, PaperPlaneTilt, PencilSimple, Plus, SidebarSimple, Star, Tag, UserCircle, WarningCircle, X } from '@phosphor-icons/react';
import { api } from './api';
import { subscribeSyncEvents } from './sync-events';
import type { Account, Contact, DeveloperToken, Draft, MailboxRole, Message } from './types';
import type { AppPreferences, ContextTarget, MailNotification, MessageStats, Notice, ShortcutBindings, WorkspaceFolder } from './app-model';
import { AccountProviderMark, ProviderIcon, providerLabel } from './components/shared';
import { BrandLogo } from './components/brand-logo';
import { AppInput } from './components/form-controls';
import { applyMessageChanges, applyMessageStatsChanges, messageTotalDelta, VirtualMessageList, MessageReader, type MessageChange } from './features/mail';
import { AddAccountModal } from './features/accounts';
import { ComposePane, DraftWorkspace, type ComposePaneHandle } from './features/compose';
import { LabelModal, NotificationsModal, SnoozeModal, WorkspaceFolderItem, WorkspaceIcon, WorkspaceModal } from './features/organize';
import { CreateApiTokenModal, CreateMcpTokenModal, TokenWorkspace } from './features/developer';
import { isBrowserRefreshShortcut, isEditableShortcutTarget, loadShortcutBindings, shortcutDefinitions, shortcutLabel, shortcutMatches, shortcutStorageKeyFor } from './features/shortcuts';
import { AppContextMenu } from './features/context-menu';
import { loadAppPreferences, preferencesStorageKeyFor, SettingsModal, type SettingsTab } from './features/settings';
import { useAuth } from './features/auth';

type View = 'inbox' | 'starred' | 'sent' | 'snoozed' | 'archive' | 'folder' | 'drafts' | 'tokens';
type MessagePage = { messages: Message[]; total: number; nextOffset: number; hasMore: boolean };
function App() {
  const { user, logout } = useAuth();
  const [realAccounts, setRealAccounts] = useState<Account[]>([]);
  const [realMessages, setRealMessages] = useState<Message[]>([]);
  const [tokens, setTokens] = useState<DeveloperToken[]>([]);
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [contacts, setContacts] = useState<Contact[]>([]);
  const [labels, setLabels] = useState<string[]>([]);
  const [ready, setReady] = useState(false);
  const localPreferencesKey = preferencesStorageKeyFor(user.id);
  const localShortcutsKey = shortcutStorageKeyFor(user.id);
  const [preferences, setPreferences] = useState<AppPreferences>(() => loadAppPreferences(localStorage, localPreferencesKey));
  const [view, setView] = useState<View>(() => loadAppPreferences(localStorage, localPreferencesKey).startupView);
  const [accountFilter, setAccountFilter] = useState('all');
  const [groupFilter, setGroupFilter] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [mailFilter, setMailFilter] = useState<'all' | 'unread' | 'attachments'>('all');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [composeMode, setComposeMode] = useState<'new' | 'reply' | 'forward' | null>(null);
  const [tokenOpen, setTokenOpen] = useState<'api' | 'mcp' | null>(null);
  const [settingsTab, setSettingsTab] = useState<SettingsTab | null>(null);
  const [shortcutBindings, setShortcutBindings] = useState<ShortcutBindings>(() => loadShortcutBindings(localStorage, localShortcutsKey));
  const [contextTarget, setContextTarget] = useState<ContextTarget | null>(null);
  const [notificationsOpen, setNotificationsOpen] = useState(false);
  const [notifications, setNotifications] = useState<MailNotification[]>([]);
  const [labelOpen, setLabelOpen] = useState(false);
  const [snoozeOpen, setSnoozeOpen] = useState(false);
  const [workspaceOpen, setWorkspaceOpen] = useState<string | null | undefined>(undefined);
  const [activeMailbox, setActiveMailbox] = useState<WorkspaceFolder | null>(null);
  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Set<string>>(() => new Set());
  const [activeLabel, setActiveLabel] = useState<string | null>(null);
  const [activeDraft, setActiveDraft] = useState<Draft | undefined>();
  const [composeAccountId, setComposeAccountId] = useState<string | undefined>();
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
  const realAccountsRef = useRef<Account[]>([]);
  const implicitSelectedIdRef = useRef<string | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const folderDiscoveryStarted = useRef(false);
  const preferencesSaveQueue = useRef<Promise<void>>(Promise.resolve());
  const composePaneRef = useRef<ComposePaneHandle | null>(null);

  const accounts = realAccounts;
  const messages = realMessages;

  const load = useCallback(async () => {
    try {
      const [accountData, tokenData, statsData, draftData, labelData, contactData] = await Promise.all([
        api<{ accounts: Account[] }>('/api/accounts'),
        api<{ tokens: DeveloperToken[] }>('/api/developer-tokens'),
        api<MessageStats>('/api/message-stats'),
        api<{ drafts: Draft[] }>('/api/drafts'),
        api<{ labels: string[] }>('/api/labels'),
        api<{ contacts: Contact[] }>('/api/contacts'),
      ]);
      setRealAccounts(accountData.accounts);
      setTokens(tokenData.tokens);
      setMessageStats(statsData);
      setDrafts(draftData.drafts);
      setLabels(labelData.labels);
      setContacts(contactData.contacts);
    } catch (error) {
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '服务连接失败' });
    }
  }, []);

  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    void api<{ preferences: AppPreferences }>('/api/preferences').then((result) => {
      setPreferences(result.preferences); setShortcutBindings(result.preferences.shortcutBindings); setView(result.preferences.startupView);
      localStorage.setItem(localPreferencesKey, JSON.stringify(result.preferences));
      localStorage.setItem(localShortcutsKey, JSON.stringify(result.preferences.shortcutBindings));
    }).catch(() => undefined);
  }, []);
  useEffect(() => { realAccountsRef.current = realAccounts; }, [realAccounts]);
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
  messageQueryRef.current = messageQuery;
  useEffect(() => {
    const applySyncChanges = (event: MessageEvent) => {
      try {
        const changes = (JSON.parse(event.data) as { payload?: { messageChanges?: MessageChange[] } }).payload?.messageChanges ?? [];
        if (changes.length === 0) return;
        const query = messageQueryRef.current;
        const currentAccounts = realAccountsRef.current;
        setRealMessages((current) => applyMessageChanges(current, changes, query, currentAccounts));
        setMessageTotal((current) => Math.max(0, current + messageTotalDelta(changes, query, currentAccounts)));
        setMessageStats((current) => applyMessageStatsChanges(current, changes, currentAccounts));
      } catch {
        // A malformed optional event payload must not interrupt the current mailbox view.
      }
    };
    return subscribeSyncEvents(['sync.completed'], applySyncChanges);
  }, []);
  const selected = messages.find((message) => message.id === (selectedId ?? implicitSelectedIdRef.current)) ?? messages[0];
  const selectedIndex = selected ? messages.findIndex((message) => message.id === selected.id) : -1;
  const activeAccount = accountFilter === 'all' ? undefined : accounts.find((account) => account.id === accountFilter);
  const visibleMessages = mailFilter === 'unread' ? messages.filter((message) => message.unread) : messages;

  useEffect(() => {
    let cancelled = false;
    messageQueryRef.current = messageQuery;
    const timer = window.setTimeout(() => {
      setMessagesLoading(true);
      void api<MessagePage>(`/api/messages?${messageQuery}&limit=60&offset=0`).then((result) => {
        if (cancelled) return;
        implicitSelectedIdRef.current = result.messages[0]?.id ?? null;
        setRealMessages(result.messages); setMessageTotal(result.total); setMessagesHasMore(result.hasMore); setSelectedId(null);
      }).catch((error) => { if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮件缓存加载失败' }); })
        .finally(() => { if (!cancelled) { setMessagesLoading(false); setReady(true); } });
    }, search.trim() ? 220 : 0);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [messageQuery, messageRevision]);

  useEffect(() => {
    if ((view !== 'sent' && view !== 'archive') || accounts.length === 0) return;
    let cancelled = false; setSyncing(true);
    void api(`/api/mailboxes/${view}/sync`, { method: 'POST' })
      .catch((error) => { if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '文件夹同步失败' }); })
      .finally(() => { if (!cancelled) setSyncing(false); });
    return () => { cancelled = true; };
  }, [view]);

  useEffect(() => {
    if (view !== 'folder' || !activeMailbox) return;
    let cancelled = false; setSyncing(true);
    void Promise.all(activeMailbox.targets.map((target) => api(`/api/accounts/${target.accountId}/mailboxes/sync`, { method: 'POST', body: JSON.stringify({ mailbox: target.path }) })))
      .catch((error) => { if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '文件夹同步失败' }); })
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
      setNotice({ kind: 'success', text: '同步任务已加入后端队列，可在“同步设置”查看进度' });
    }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '同步失败' }); }
    finally { setSyncing(false); }
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
    if (message.unread === unread) return;
    setRealMessages((current) => current.map((item) => item.id === message.id ? { ...item, unread } : item));
    if (mailFilter === 'unread') setMessageTotal((current) => Math.max(0, current + (unread ? 1 : -1)));
    if (message.mailboxRole === 'inbox') adjustMessageStats(message.accountId, 0, unread ? 1 : -1);
    try {
      await api(`/api/messages/${message.id}`, { method: 'PATCH', body: JSON.stringify({ unread }) });
      setNotice({ kind: 'success', text: unread ? '邮件已标记为未读' : '邮件已标记为已读' });
    } catch (error) {
      setRealMessages((current) => current.map((item) => item.id === message.id ? { ...item, unread: !unread } : item));
      if (mailFilter === 'unread') setMessageTotal((current) => Math.max(0, current + (unread ? -1 : 1)));
      if (message.mailboxRole === 'inbox') adjustMessageStats(message.accountId, 0, unread ? -1 : 1);
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '已读状态更新失败' });
    }
  }

  async function markSelectedUnread() { if (selected) await setMessageUnread(selected, true); }

  async function toggleSelectedFlag(target = selected) {
    if (!target) return;
    const flagged = !target.flagged;
    setRealMessages((current) => current.map((message) => message.id === target.id ? { ...message, flagged } : message));
    try {
      await api(`/api/messages/${target.id}`, { method: 'PATCH', body: JSON.stringify({ flagged }) });
      if (view === 'starred' && !flagged) setMessageRevision((value) => value + 1);
    } catch (error) {
      setRealMessages((current) => current.map((message) => message.id === target.id ? { ...message, flagged: !flagged } : message));
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

  async function selectMessage(id: string) {
    if (composeMode) await composePaneRef.current?.close();
    setSelectedId(id);
    const message = realMessages.find((item) => item.id === id);
    if (!message?.unread || !preferences.markReadOnOpen) return;

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

  async function moveSelected(destination: 'archive' | 'trash', target = selected) {
    if (!target || messageActionBusy) return;
    const message = target;
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

  function focusSearch() {
    const input = document.querySelector<HTMLInputElement>('.search-box input') ?? searchInputRef.current;
    input?.focus(); input?.select();
  }

  function openCompose(accountId?: string) {
    setComposeAccountId(accountId); setActiveDraft(undefined); setComposeMode('new'); setView('inbox'); setSidebarOpen(false);
  }

  function saveShortcutBindings(bindings: ShortcutBindings) {
    setShortcutBindings(bindings);
    localStorage.setItem(localShortcutsKey, JSON.stringify(bindings));
    savePreferences({ ...preferences, shortcutBindings: bindings });
    setNotice({ kind: 'success', text: '快捷键已保存' });
  }

  function savePreferences(next: AppPreferences) {
    setPreferences(next);
    localStorage.setItem(localPreferencesKey, JSON.stringify(next));
    preferencesSaveQueue.current = preferencesSaveQueue.current.then(async () => {
      const result = await api<{ preferences: AppPreferences }>('/api/preferences', { method: 'PATCH', body: JSON.stringify(next) });
      setPreferences(result.preferences);
      setShortcutBindings(result.preferences.shortcutBindings);
      localStorage.setItem(localPreferencesKey, JSON.stringify(result.preferences));
      localStorage.setItem(localShortcutsKey, JSON.stringify(result.preferences.shortcutBindings));
    }).catch((error) => {
      setNotice({ kind: 'error', text: error instanceof Error ? `设置已保存在本机，但服务端同步失败：${error.message}` : '设置已保存在本机，但服务端同步失败' });
    });
  }

  async function syncAccount(accountId: string) {
    setSyncing(true);
    try { await api(`/api/accounts/${accountId}/sync`, { method: 'POST' }); setNotice({ kind: 'success', text: '邮箱已加入后端同步队列' }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮箱同步失败' }); }
    finally { setSyncing(false); }
  }

  async function syncWorkspace(group: string) {
    setSyncing(true);
    try { await Promise.all(accounts.filter((account) => account.group === group).map((account) => api(`/api/accounts/${account.id}/sync`, { method: 'POST' }))); setNotice({ kind: 'success', text: `${group} 已加入后端同步队列` }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '工作空间同步失败' }); }
    finally { setSyncing(false); }
  }

  async function syncFolder(folder: WorkspaceFolder) {
    setSyncing(true);
    try { await Promise.all(folder.targets.map((target) => api(`/api/accounts/${target.accountId}/mailboxes/sync`, { method: 'POST', body: JSON.stringify({ mailbox: target.path }) }))); setNotice({ kind: 'success', text: `${folder.name} 已加入后端同步队列` }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '文件夹同步失败' }); }
    finally { setSyncing(false); }
  }

  async function openMessageContext(message: Message, point: { x: number; y: number }) {
    if (composeMode) await composePaneRef.current?.close();
    setSelectedId(message.id); setContextTarget({ kind: 'message', messageId: message.id, ...point });
  }

  useEffect(() => {
    const onShortcut = (event: KeyboardEvent) => {
      if (event.defaultPrevented || addOpen || tokenOpen || settingsTab || notificationsOpen || labelOpen || snoozeOpen || workspaceOpen !== undefined) return;
      if (isBrowserRefreshShortcut(event)) return;
      const definition = shortcutDefinitions.find(({ id }) => shortcutMatches(event, shortcutBindings[id]));
      if (!definition) return;
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

  const scopeTitle = activeMailbox?.name ?? (activeLabel ? `标签 · ${activeLabel}` : view === 'starred' ? '星标邮件' : view === 'sent' ? '已发送' : view === 'snoozed' ? '稍后处理' : view === 'archive' ? '归档' : '统一收件箱');

  return <div className={`app-shell ${sidebarCollapsed ? 'sidebar-collapsed' : ''}`}>
    {notice && <div className={`toast toast-${notice.kind}`}>{notice.kind === 'success' ? <Check size={18} /> : <WarningCircle size={18} />}<span>{notice.text}</span></div>}

    <aside className="account-rail" aria-label="邮箱账户">
      <button className="brand-mark" aria-label="iMail"><BrandLogo /></button>
      <div className="rail-accounts">
        <button title="聚合所有邮箱" aria-label="聚合所有邮箱" className={`rail-avatar rail-all ${accountFilter === 'all' ? 'active' : ''}`} onClick={() => selectScope('inbox')} onContextMenu={(event) => { event.preventDefault(); setContextTarget({ kind: 'background', x: event.clientX, y: event.clientY }); }}><Tray size={20} /></button>
        {accounts.map((account) =>
          <button key={account.id} title={`${providerLabel[account.provider]} · ${account.displayName} · ${account.email}`} aria-label={`${providerLabel[account.provider]}，${account.displayName}，${account.email}`} className={`rail-avatar rail-account provider-${account.provider} ${accountFilter === account.id ? 'active' : ''}`} style={{ '--avatar-color': account.color } as React.CSSProperties} onClick={() => selectScope('inbox', account.id)} onContextMenu={(event) => { event.preventDefault(); setContextTarget({ kind: 'account', accountId: account.id, x: event.clientX, y: event.clientY }); }}>
            <ProviderIcon provider={account.provider} /><span className={`status status-${account.status}`} />
          </button>)}
        <button title="添加邮箱" aria-label="添加邮箱" className="rail-avatar rail-add" onClick={() => setAddOpen(true)}><Plus size={19} /></button>
      </div>
      <button title="设置" aria-label="打开设置" className="rail-avatar rail-settings" onClick={() => setSettingsTab('general')}><Gear size={19} /></button>
    </aside>

    <aside className={`primary-sidebar ${sidebarOpen ? 'mobile-open' : ''}`}>
      <div className="sidebar-heading"><div><strong>iMail</strong><span>统一通信工作台</span></div><button className="mobile-close" aria-label="关闭侧栏" onClick={() => setSidebarOpen(false)}><X size={20} /></button></div>
      <Button appearance="primary" icon={<PencilSimple size={18} />} className="compose-button" onClick={() => openCompose(activeAccount?.id)}>写邮件</Button>
      <div className="mobile-account-controls" aria-label="移动端邮箱账户">
        <button className={accountFilter === 'all' ? 'active' : ''} onClick={() => selectScope('inbox')}><Tray size={18} /><span><strong>全部邮箱</strong><small>{accounts.length} 个账户</small></span></button>
        {accounts.map((account) => <button key={account.id} className={accountFilter === account.id ? 'active' : ''} onClick={() => selectScope('inbox', account.id)}><AccountProviderMark provider={account.provider} /><span><strong>{account.displayName}</strong><small>{account.email}</small></span></button>)}
        <div><button onClick={() => { setAddOpen(true); setSidebarOpen(false); }}><Plus size={16} />添加邮箱</button><button onClick={() => { setSettingsTab('general'); setSidebarOpen(false); }}><Gear size={16} />设置</button></div>
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
              <div className="workspace-row"><button className={groupFilter === group ? 'active' : ''} onClick={() => selectScope('inbox', 'all', group)} onContextMenu={(event) => { event.preventDefault(); setContextTarget({ kind: 'workspace', group, x: event.clientX, y: event.clientY }); }}><WorkspaceIcon icon={icon} size={17} /><span>{group}</span><b>{messageStats.byGroup.find((item) => item.group === group)?.unread || ''}</b></button><button className="workspace-edit" title={`编辑工作空间 ${group}`} aria-label={`编辑工作空间 ${group}`} onClick={() => setWorkspaceOpen(group)}><PencilSimple size={14} /></button></div>
              <div className="workspace-mailboxes">{visibleFolders.map((folder) => <WorkspaceFolderItem key={folder.name.toLocaleLowerCase()} folder={folder} active={view === 'folder' && activeMailbox?.group === group && activeMailbox.name.toLocaleLowerCase() === folder.name.toLocaleLowerCase()} onSelect={selectMailbox} onContextMenu={(target, point) => setContextTarget({ kind: 'folder', folder: target, ...point })} />)}{folders.length > 3 && <button className={`workspace-folder-toggle ${expanded ? 'is-expanded' : ''}`} onClick={() => setExpandedWorkspaces((current) => { const next = new Set(current); if (expanded) next.delete(group); else next.add(group); return next; })}><CaretDown size={14} /><span>{expanded ? '收起' : `更多 ${folders.length - 3}`}</span></button>}</div>
            </div>;
          })}
        </nav>
      </section>
      {labels.length > 0 && <><div className="section-label"><span>邮件标签</span></div><nav className="nav-block groups label-nav">{labels.map((label) => <button key={label} data-icon-tone="info" className={activeLabel === label ? 'active' : ''} onClick={() => selectLabel(label)}><Tag size={16} /><span>{label}</span></button>)}</nav></>}
      <div className="sidebar-spacer" />
      <button className={`developer-entry ${view === 'tokens' ? 'active' : ''}`} onClick={() => selectScope('tokens')}><Code size={19} /><span><strong>外部接入</strong><small>MCP 与邮件 API</small></span><ArrowRight size={16} /></button>
      <button className="user-strip" type="button" onClick={() => void logout()} title="退出并切换应用账号"><UserCircle size={32} weight="duotone" /><span><strong>{user.displayName}</strong><small>{user.login} · 切换账号</small></span><CaretDown size={15} /></button>
    </aside>

    <main className="workspace">
      <header className="topbar">
        <button className="sidebar-trigger desktop-sidebar-trigger" title={sidebarCollapsed ? '展开侧栏' : '收起侧栏'} aria-label={sidebarCollapsed ? '展开侧栏' : '收起侧栏'} aria-expanded={!sidebarCollapsed} onClick={() => setSidebarCollapsed((current) => !current)}><SidebarSimple size={20} /></button>
        <button className="sidebar-trigger mobile-sidebar-trigger" title="打开侧栏" aria-label="打开侧栏" aria-expanded={sidebarOpen} onClick={() => setSidebarOpen(true)}><SidebarSimple size={20} /></button>
        <AppInput className="search-box" contentBefore={<MagnifyingGlass size={18} />} contentAfter={<kbd>{shortcutLabel(shortcutBindings.focusSearch)}</kbd>} ref={searchInputRef} value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索当前范围内的邮件" aria-label="搜索当前范围内的邮件" />
        <button data-icon-tone="primary" className={`sync-button ${syncing ? 'is-syncing' : ''}`} title="立即同步当前范围" onClick={() => void syncAll()}><ArrowClockwise size={18} /><span>{syncing ? '已入队' : '立即同步'}</span></button>
        <button data-icon-tone="info" className="icon-button" title="通知中心" aria-label="打开通知中心" onClick={() => void openNotifications()}><Bell size={19} /></button>
      </header>

      {view === 'tokens' ? <TokenWorkspace accounts={realAccounts} tokens={tokens} onCreateApi={() => setTokenOpen('api')} onCreateMcp={() => setTokenOpen('mcp')} onReload={load} setNotice={setNotice} /> :
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
            <VirtualMessageList messages={visibleMessages} accounts={accounts} selectedId={selected?.id} ready={ready} loading={messagesLoading} hasMore={messagesHasMore} onSelect={selectMessage} onContextMenu={(message, point) => void openMessageContext(message, point)} onBackgroundContextMenu={(point) => setContextTarget({ kind: 'background', ...point })} onLoadMore={loadMoreMessages} onAddAccount={() => setAddOpen(true)} />
          </section>}
          {composeMode ? <ComposePane ref={composePaneRef} key={`${composeMode}-${activeDraft?.id ?? selected?.id ?? composeAccountId ?? 'new'}`} accounts={realAccounts} contacts={contacts} mode={composeMode} initialAccountId={composeAccountId} original={composeMode === 'new' ? undefined : selected} draft={activeDraft} onClose={() => { setComposeMode(null); setComposeAccountId(undefined); setActiveDraft(undefined); }} onDraftSaved={(saved) => { setDrafts((current) => [saved, ...current.filter((item) => item.id !== saved.id)]); }} onSent={async () => { setComposeMode(null); setComposeAccountId(undefined); setActiveDraft(undefined); await load(); setNotice({ kind: 'success', text: '邮件已发送' }); }} /> : view === 'drafts' ? <section className="composer-pane composer-welcome"><PencilSimple size={48} weight="duotone" /><h2>选择草稿继续编辑</h2><p>修改会自动保存，也可以直接新建一封邮件。</p><button onClick={() => openCompose(activeAccount?.id)}>新建邮件</button></section> : <MessageReader message={selected} account={selected ? accounts.find((item) => item.id === selected.accountId) : undefined} defaultBodyView={preferences.defaultMessageView} onReply={() => { setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('reply'); }} onForward={() => { setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('forward'); }} onCloseMobile={() => setSelectedId(null)} onContextMenu={(message, point) => void openMessageContext(message, point)}
            onToggleFlag={() => void toggleSelectedFlag()}
            onSnooze={() => setSnoozeOpen(true)} onManageLabels={() => setLabelOpen(true)} onMarkUnread={() => void markSelectedUnread()}
            onArchive={() => void moveSelected('archive')} onDelete={() => void moveSelected('trash')} actionBusy={messageActionBusy}
            onPrevious={() => { if (selectedIndex > 0) selectMessage(messages[selectedIndex - 1].id); }}
            onNext={() => { if (selectedIndex >= 0 && selectedIndex < messages.length - 1) selectMessage(messages[selectedIndex + 1].id); }}
            hasPrevious={selectedIndex > 0} hasNext={selectedIndex >= 0 && selectedIndex < messages.length - 1} />}
        </div>}
    </main>

    {addOpen && <AddAccountModal accounts={accounts} onClose={() => setAddOpen(false)} onAdded={async (result) => { setAddOpen(false); await load(); setMessageRevision((value) => value + 1); setNotice(result?.warning ? { kind: 'error', text: `授权已保存，连接验证失败：${result.warning}` } : { kind: 'success', text: '邮箱已接入，正在准备统一收件箱' }); }} />}
    {tokenOpen === 'api' && <CreateApiTokenModal accounts={realAccounts} onClose={() => setTokenOpen(null)} onCreated={load} />}
    {tokenOpen === 'mcp' && <CreateMcpTokenModal onClose={() => setTokenOpen(null)} onCreated={load} />}
    {settingsTab && <SettingsModal initialTab={settingsTab} accounts={realAccounts} preferences={preferences} bindings={shortcutBindings} onPreferencesChange={savePreferences} onBindingsChange={saveShortcutBindings} onAddAccount={() => { setSettingsTab(null); setAddOpen(true); }} onClose={() => setSettingsTab(null)} onReload={load} setNotice={setNotice} />}
    {notificationsOpen && <NotificationsModal notifications={notifications} accounts={accounts} onClose={() => setNotificationsOpen(false)} onOpenMessage={(notification) => { setNotificationsOpen(false); if (notification.accountId) setAccountFilter(notification.accountId); setView('inbox'); setSelectedId(notification.messageId ?? null); }} />}
    {labelOpen && selected && <LabelModal message={selected} knownLabels={labels} onClose={() => setLabelOpen(false)} onSave={(next) => { setLabelOpen(false); void updateSelectedLocal({ labels: next }, '邮件标签已更新'); }} />}
    {snoozeOpen && selected && <SnoozeModal onClose={() => setSnoozeOpen(false)} onSave={(until) => { setSnoozeOpen(false); void updateSelectedLocal({ snoozedUntil: until }, until ? '邮件已移到稍后处理' : '邮件已返回收件箱'); }} />}
    {workspaceOpen !== undefined && <WorkspaceModal accounts={accounts} workspace={workspaceOpen ?? undefined} onClose={() => setWorkspaceOpen(undefined)} onSaved={async () => { setWorkspaceOpen(undefined); await load(); setNotice({ kind: 'success', text: '工作空间已更新' }); }} />}
    {contextTarget && <AppContextMenu target={contextTarget} bindings={shortcutBindings} messages={messages} accounts={accounts} activeAccountId={activeAccount?.id} onClose={() => setContextTarget(null)} actions={{
      openMessage: (id) => void selectMessage(id),
      reply: () => { setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('reply'); },
      forward: () => { setComposeAccountId(undefined); setActiveDraft(undefined); setComposeMode('forward'); },
      toggleStar: (message) => void toggleSelectedFlag(message), setUnread: (message, unread) => void setMessageUnread(message, unread),
      snooze: () => setSnoozeOpen(true), labels: () => setLabelOpen(true), archive: (message) => void moveSelected('archive', message), delete: (message) => void moveSelected('trash', message),
      openAccount: (id) => selectScope('inbox', id), compose: openCompose, syncAccount: (id) => void syncAccount(id), accountSettings: () => setSettingsTab('accounts'),
      openWorkspace: (group) => selectScope('inbox', 'all', group), syncWorkspace: (group) => void syncWorkspace(group), editWorkspace: setWorkspaceOpen,
      openFolder: selectMailbox, syncFolder: (folder) => void syncFolder(folder), syncCurrent: () => void syncAll(), shortcutSettings: () => setSettingsTab('shortcuts'),
    }} />}
  </div>;
}

export default App;
