import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { Button, Tooltip } from '@fluentui/react-components';
import {
  AddressBook, Archive, ArrowClockwise, ArrowLeft, ArrowRight, Bell, CaretDown, Check,
  Clock, Code, Copy, Envelope, File, Gear, Tray, Key, MagnifyingGlass, PaperPlaneTilt,
  PencilSimple, Plus, SidebarSimple, Star, Tag, Trash, UserCircle, WarningCircle, X,
} from '@phosphor-icons/react';
import { api } from './api';
import type { Account, DeveloperToken, Message, ProviderId } from './types';
import { virtualRange } from './virtual';

type View = 'inbox' | 'starred' | 'tokens';
type Notice = { kind: 'success' | 'error'; text: string } | null;
type MessagePage = { messages: Message[]; total: number; nextOffset: number; hasMore: boolean };

const providerLabel: Record<ProviderId, string> = { outlook: 'Outlook', gmail: 'Gmail', qq: 'QQ', yahoo: 'Yahoo', hotmail: 'Hotmail', icloud: 'iCloud', custom: 'IMAP' };
const providers: Array<{ id: ProviderId; name: string; mark: string; oauthKey?: 'google' | 'microsoft' | 'yahoo'; helpUrl?: string }> = [
  { id: 'outlook', name: 'Outlook', mark: 'O', oauthKey: 'microsoft' }, { id: 'gmail', name: 'Gmail', mark: 'G', oauthKey: 'google' },
  { id: 'qq', name: 'QQ 邮箱', mark: 'Q', helpUrl: 'https://service.mail.qq.com/' }, { id: 'yahoo', name: 'Yahoo', mark: 'Y', oauthKey: 'yahoo', helpUrl: 'https://login.yahoo.com/account/security' },
  { id: 'hotmail', name: 'Hotmail', mark: 'H', oauthKey: 'microsoft' }, { id: 'icloud', name: 'iCloud', mark: 'i', helpUrl: 'https://account.apple.com/account/manage' },
  { id: 'custom', name: '其他 IMAP', mark: '+' },
];

function initials(value: string) {
  const parts = value.trim().split(/\s+/);
  return (parts.length > 1 ? parts.map((part) => part[0]).join('') : value.slice(0, 2)).toUpperCase();
}

function relativeTime(value: string) {
  const diff = Date.now() - new Date(value).getTime();
  if (diff < 60 * 60_000) return `${Math.max(1, Math.floor(diff / 60_000))} 分钟前`;
  if (diff < 24 * 60 * 60_000) return `${Math.floor(diff / 3600_000)} 小时前`;
  if (diff < 48 * 60 * 60_000) return '昨天';
  return new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric' }).format(new Date(value));
}

function Overlay({ children, onClose, wide = false }: { children: ReactNode; onClose: () => void; wide?: boolean }) {
  return <div className="overlay" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section className={`modal ${wide ? 'modal-wide' : ''}`} role="dialog" aria-modal="true">{children}</section>
  </div>;
}

function App() {
  const [realAccounts, setRealAccounts] = useState<Account[]>([]);
  const [realMessages, setRealMessages] = useState<Message[]>([]);
  const [tokens, setTokens] = useState<DeveloperToken[]>([]);
  const [ready, setReady] = useState(false);
  const [view, setView] = useState<View>('inbox');
  const [accountFilter, setAccountFilter] = useState('all');
  const [groupFilter, setGroupFilter] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [mailFilter, setMailFilter] = useState<'all' | 'unread' | 'attachments'>('all');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [composeOpen, setComposeOpen] = useState(false);
  const [tokenOpen, setTokenOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  const [syncing, setSyncing] = useState(false);
  const [messageTotal, setMessageTotal] = useState(0);
  const [messagesHasMore, setMessagesHasMore] = useState(false);
  const [messagesLoading, setMessagesLoading] = useState(false);
  const [messageRevision, setMessageRevision] = useState(0);
  const messageQueryRef = useRef('');

  const accounts = realAccounts;
  const messages = realMessages;

  async function load() {
    try {
      const [accountData, tokenData] = await Promise.all([
        api<{ accounts: Account[] }>('/api/accounts'),
        api<{ tokens: DeveloperToken[] }>('/api/developer-tokens'),
      ]);
      setRealAccounts(accountData.accounts);
      setTokens(tokenData.tokens);
    } catch (error) {
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '服务连接失败' });
    }
  }

  useEffect(() => { void load(); }, []);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 4200);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const groups = useMemo(() => Array.from(new Set(accounts.map((account) => account.group))), [accounts]);
  const messageQuery = useMemo(() => {
    const params = new URLSearchParams();
    if (accountFilter !== 'all') params.set('accountId', accountFilter);
    if (groupFilter) params.set('group', groupFilter);
    if (search.trim()) params.set('q', search.trim());
    if (view === 'starred') params.set('flagged', 'true');
    if (mailFilter === 'unread') params.set('unread', 'true');
    if (mailFilter === 'attachments') params.set('hasAttachments', 'true');
    return params.toString();
  }, [accountFilter, groupFilter, search, view, mailFilter]);
  const selected = messages.find((message) => message.id === selectedId) ?? messages[0];

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
    if (!selected || selected.text !== undefined) return;
    let cancelled = false;
    void api<{ message: Message }>(`/api/messages/${selected.id}`).then(({ message }) => {
      if (!cancelled) setRealMessages((current) => current.map((item) => item.id === message.id ? message : item));
    }).catch((error) => { if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮件正文加载失败' }); });
    return () => { cancelled = true; };
  }, [selected?.id, selected?.text]);

  const loadMoreMessages = useCallback(async () => {
    if (messagesLoading || !messagesHasMore) return;
    const queryAtStart = messageQuery;
    setMessagesLoading(true);
    try {
      const result = await api<MessagePage>(`/api/messages?${queryAtStart}&limit=60&offset=${realMessages.length}`);
      if (messageQueryRef.current !== queryAtStart) return;
      setRealMessages((current) => [...current, ...result.messages.filter((message) => !current.some((item) => item.id === message.id))]);
      setMessageTotal(result.total); setMessagesHasMore(result.hasMore);
    } catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '加载更多邮件失败' }); }
    finally { setMessagesLoading(false); }
  }, [messageQuery, messagesHasMore, messagesLoading, realMessages.length]);

  async function syncAll() {
    if (realAccounts.length === 0) { setAddOpen(true); return; }
    setSyncing(true);
    try { await api('/api/sync', { method: 'POST' }); await load(); setMessageRevision((value) => value + 1); setNotice({ kind: 'success', text: '缓存已更新' }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '同步失败' }); }
    finally { setSyncing(false); }
  }

  function selectScope(nextView: View, nextAccount = 'all', nextGroup: string | null = null) {
    setView(nextView); setAccountFilter(nextAccount); setGroupFilter(nextGroup); setSidebarOpen(false); setSelectedId(null);
  }

  return <div className="app-shell">
    {notice && <div className={`toast toast-${notice.kind}`}>{notice.kind === 'success' ? <Check size={18} /> : <WarningCircle size={18} />}<span>{notice.text}</span></div>}

    <aside className="account-rail" aria-label="邮箱账户">
      <button className="brand-mark" aria-label="iMail"><img src="/brand/imail-app-icon.png" alt="" /></button>
      <div className="rail-accounts">
        <Tooltip content="聚合所有邮箱" relationship="label"><button className={`rail-avatar rail-all ${accountFilter === 'all' ? 'active' : ''}`} onClick={() => selectScope('inbox')}><Tray size={20} /></button></Tooltip>
        {accounts.map((account) => <Tooltip key={account.id} content={`${account.displayName} · ${account.email}`} relationship="label">
          <button className={`rail-avatar ${accountFilter === account.id ? 'active' : ''}`} style={{ '--avatar-color': account.color } as React.CSSProperties} onClick={() => selectScope('inbox', account.id)}>
            {initials(account.displayName)}<span className={`status status-${account.status}`} />
          </button>
        </Tooltip>)}
        <Tooltip content="添加邮箱" relationship="label"><button className="rail-avatar rail-add" onClick={() => setAddOpen(true)}><Plus size={19} /></button></Tooltip>
      </div>
      <Tooltip content="邮箱设置" relationship="label"><button className="rail-avatar rail-settings" onClick={() => setSettingsOpen(true)}><Gear size={19} /></button></Tooltip>
    </aside>

    <aside className={`primary-sidebar ${sidebarOpen ? 'mobile-open' : ''}`}>
      <div className="sidebar-heading"><div><strong>iMail</strong><span>统一通信工作台</span></div><button className="mobile-close" onClick={() => setSidebarOpen(false)}><X size={20} /></button></div>
      <Button appearance="primary" icon={<PencilSimple size={18} />} className="compose-button" onClick={() => setComposeOpen(true)}>写邮件</Button>
      <nav className="nav-block">
        <button className={view === 'inbox' && !groupFilter ? 'active' : ''} onClick={() => selectScope('inbox')}><Tray size={19} /><span>统一收件箱</span><b>{messages.filter((m) => m.unread).length}</b></button>
        <button className={view === 'starred' ? 'active' : ''} onClick={() => selectScope('starred')}><Star size={19} /><span>已加星标</span></button>
        <button><PaperPlaneTilt size={19} /><span>已发送</span></button>
        <button><Clock size={19} /><span>稍后处理</span></button>
        <button><Archive size={19} /><span>归档</span></button>
      </nav>
      <div className="section-label"><span>工作空间</span><button><Plus size={15} /></button></div>
      <nav className="nav-block groups">
        {groups.map((group, index) => <button key={group} className={groupFilter === group ? 'active' : ''} onClick={() => selectScope('inbox', 'all', group)}><span className={`group-symbol group-${index % 4}`} /><span>{group}</span><b>{messages.filter((message) => accounts.find((account) => account.id === message.accountId)?.group === group && message.unread).length || ''}</b></button>)}
      </nav>
      <div className="sidebar-spacer" />
      <button className={`developer-entry ${view === 'tokens' ? 'active' : ''}`} onClick={() => selectScope('tokens')}><Code size={19} /><span><strong>开发者网关</strong><small>Token 与邮件 API</small></span><ArrowRight size={16} /></button>
      <div className="user-strip"><UserCircle size={32} weight="duotone" /><span><strong>本地工作区</strong><small>数据仅存储在本机</small></span><CaretDown size={15} /></div>
    </aside>

    <main className="workspace">
      <header className="topbar">
        <button className="sidebar-trigger" onClick={() => setSidebarOpen(true)}><SidebarSimple size={20} /></button>
        <div className="search-box"><MagnifyingGlass size={18} /><input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索所有邮箱中的邮件" /><kbd>⌘ K</kbd></div>
        <button className={`sync-button ${syncing ? 'is-syncing' : ''}`} onClick={() => void syncAll()}><ArrowClockwise size={18} /><span>{syncing ? '同步中' : '同步'}</span></button>
        <button className="icon-button"><Bell size={19} /></button>
      </header>

      {view === 'tokens' ? <TokenWorkspace accounts={realAccounts} tokens={tokens} onCreate={() => setTokenOpen(true)} onReload={load} setNotice={setNotice} /> :
        <div className="mail-layout">
          <section className="message-pane">
            <div className="pane-title"><div><p>{groupFilter ?? (accountFilter === 'all' ? (view === 'starred' ? '星标邮件' : '统一收件箱') : accounts.find((account) => account.id === accountFilter)?.displayName)}</p><span>{messageTotal} 封邮件</span></div><button><Tag size={18} /></button></div>
            <div className="message-filters"><button className={mailFilter === 'all' ? 'active' : ''} onClick={() => setMailFilter('all')}>全部</button><button className={mailFilter === 'unread' ? 'active' : ''} onClick={() => setMailFilter('unread')}>未读</button><button className={mailFilter === 'attachments' ? 'active' : ''} onClick={() => setMailFilter('attachments')}>有附件</button></div>
            <VirtualMessageList messages={messages} accounts={accounts} selectedId={selected?.id} ready={ready} loading={messagesLoading} hasMore={messagesHasMore} onSelect={setSelectedId} onLoadMore={loadMoreMessages} onAddAccount={() => setAddOpen(true)} />
          </section>
          <MessageReader message={selected} account={selected ? accounts.find((item) => item.id === selected.accountId) : undefined} onReply={() => setComposeOpen(true)} />
        </div>}
    </main>

    {addOpen && <AddAccountModal onClose={() => setAddOpen(false)} onAdded={async () => { setAddOpen(false); await load(); setMessageRevision((value) => value + 1); setNotice({ kind: 'success', text: '邮箱已接入，正在准备统一收件箱' }); }} />}
    {composeOpen && <ComposeModal accounts={realAccounts} reply={selected} onClose={() => setComposeOpen(false)} onSent={() => { setComposeOpen(false); setNotice({ kind: 'success', text: '邮件已发送' }); }} />}
    {tokenOpen && <CreateTokenModal accounts={realAccounts} onClose={() => setTokenOpen(false)} onCreated={async () => { await load(); }} />}
    {settingsOpen && <AccountSettingsModal accounts={realAccounts} onClose={() => setSettingsOpen(false)} onReload={load} setNotice={setNotice} />}
  </div>;
}

const MESSAGE_ROW_HEIGHT = 98;
const MESSAGE_OVERSCAN = 6;

function VirtualMessageList({ messages, accounts, selectedId, ready, loading, hasMore, onSelect, onLoadMore, onAddAccount }: {
  messages: Message[]; accounts: Account[]; selectedId?: string; ready: boolean; loading: boolean; hasMore: boolean;
  onSelect: (id: string) => void; onLoadMore: () => void | Promise<void>; onAddAccount: () => void;
}) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(600);

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    const update = () => setViewportHeight(element.clientHeight);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (viewportRef.current) viewportRef.current.scrollTop = 0;
    setScrollTop(0);
  }, [messages[0]?.id]);

  const { start, end } = virtualRange(messages.length, scrollTop, viewportHeight, MESSAGE_ROW_HEIGHT, MESSAGE_OVERSCAN);

  useEffect(() => {
    if (hasMore && !loading && scrollTop + viewportHeight >= messages.length * MESSAGE_ROW_HEIGHT - MESSAGE_ROW_HEIGHT * 8) void onLoadMore();
  }, [hasMore, loading, messages.length, onLoadMore, scrollTop, viewportHeight]);

  return <div className="message-list virtual-message-list" ref={viewportRef} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
    {!ready ? Array.from({ length: 6 }).map((_, index) => <div className="message-skeleton" key={index}><i /><span /><b /></div>) : messages.length === 0 ? <div className="empty-state"><Tray size={42} weight="duotone" /><h3>{accounts.length === 0 ? '还没有接入邮箱' : '这里暂时很安静'}</h3><p>{accounts.length === 0 ? '点击左侧加号，连接你的第一个邮箱。' : '换一个邮箱或清除搜索条件试试。'}</p>{accounts.length === 0 && <button onClick={onAddAccount}>添加邮箱</button>}</div> : <div className="virtual-message-space" style={{ height: messages.length * MESSAGE_ROW_HEIGHT }}>
      {messages.slice(start, end).map((message, visibleIndex) => {
        const index = start + visibleIndex;
        const account = accounts.find((item) => item.id === message.accountId);
        const color = account?.color ?? '#66857d';
        return <button key={message.id} style={{ top: index * MESSAGE_ROW_HEIGHT, height: MESSAGE_ROW_HEIGHT }} className={`message-row virtual-message-row ${selectedId === message.id ? 'selected' : ''} ${message.unread ? 'unread' : ''}`} onClick={() => onSelect(message.id)}>
          <span className="sender-avatar" style={{ '--avatar-color': color } as React.CSSProperties}>{initials(message.from.name || message.from.address)}</span>
          <span className="message-copy"><span className="message-meta"><strong>{message.from.name || message.from.address}</strong><time>{relativeTime(message.date)}</time></span><b>{message.subject}</b><span>{message.preview}</span><small><i style={{ background: color }} />{account?.displayName ?? '邮箱'}{message.hasAttachments && <><File size={13} />附件</>}</small></span>
          {message.flagged && <Star className="row-star" size={15} weight="fill" />}
        </button>;
      })}
    </div>}
    {loading && ready && <div className="message-loading">正在读取本地缓存…</div>}
  </div>;
}

function MessageReader({ message, account, onReply }: { message?: Message; account?: Account; onReply: () => void }) {
  if (!message || !account) return <section className="reader empty-reader"><Envelope size={54} weight="duotone" /><h2>选择一封邮件开始阅读</h2><p>来自所有账户的邮件都会汇总在这里。</p></section>;
  return <article className="reader">
    <div className="reader-actions"><div><button><Archive size={18} /></button><button><Trash size={18} /></button><button><Clock size={18} /></button></div><div><button><ArrowLeft size={18} /></button><button><ArrowRight size={18} /></button></div></div>
    <div className="reader-content">
      <div className="reader-context"><span style={{ '--account-color': account.color } as React.CSSProperties}>{account.displayName}</span><span>{account.group}</span></div>
      <h1>{message.subject}</h1>
      <div className="sender-line"><span className="sender-avatar large" style={{ '--avatar-color': account.color } as React.CSSProperties}>{initials(message.from.name || message.from.address)}</span><span><strong>{message.from.name || message.from.address}</strong><small>{message.from.address} 发给 {message.to[0]?.address || account.email}</small></span><time>{new Intl.DateTimeFormat('zh-CN', { month: 'long', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(message.date))}</time><button><Star size={18} weight={message.flagged ? 'fill' : 'regular'} /></button><button><CaretDown size={16} /></button></div>
      <div className={`mail-body ${message.text === undefined ? 'mail-body-loading' : ''}`}>{message.text === undefined ? <p>正在从本地缓存加载正文…</p> : message.text.split('\n').map((line, index) => <p key={index}>{line || <br />}</p>)}</div>
      {message.attachments.length > 0 && <div className="attachments"><p>{message.attachments.length} 个附件</p>{message.attachments.map((attachment) => <button key={attachment.filename}><File size={23} weight="duotone" /><span><strong>{attachment.filename}</strong><small>{(attachment.size / 1024 / 1024).toFixed(1)} MB</small></span></button>)}</div>}
      <div className="reply-actions"><Button appearance="primary" icon={<ArrowLeft size={17} />} onClick={onReply}>回复</Button><Button appearance="outline" icon={<ArrowRight size={17} />}>转发</Button></div>
    </div>
  </article>;
}

function AddAccountModal({ onClose, onAdded }: { onClose: () => void; onAdded: () => void | Promise<void> }) {
  const [provider, setProvider] = useState<ProviderId>('outlook');
  const [advanced, setAdvanced] = useState(false);
  const [manualMode, setManualMode] = useState(false);
  const [oauthCatalog, setOauthCatalog] = useState<Array<{ id: string; configured: boolean; redirectUri: string; configurationHint: string }>>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const popupRef = useRef<Window | null>(null);
  const selectedProvider = providers.find((item) => item.id === provider)!;
  const oauthStatus = selectedProvider.oauthKey ? oauthCatalog.find((item) => item.id === selectedProvider.oauthKey) : undefined;
  const usesOAuth = Boolean(selectedProvider.oauthKey && !manualMode);

  useEffect(() => {
    void api<{ oauth: Array<{ id: string; configured: boolean; redirectUri: string; configurationHint: string }> }>('/api/providers')
      .then((result) => setOauthCatalog(result.oauth))
      .catch((value) => setError(value instanceof Error ? value.message : '无法读取 OAuth 配置'));
  }, []);

  useEffect(() => {
    const receive = (event: MessageEvent) => {
      if (event.source !== popupRef.current || event.data?.source !== 'imail-oauth') return;
      setBusy(false);
      popupRef.current = null;
      if (event.data.success) void onAdded();
      else setError(event.data.message || 'OAuth 登录未完成');
    };
    window.addEventListener('message', receive);
    return () => window.removeEventListener('message', receive);
  }, [onAdded]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('');
    const form = new FormData(event.currentTarget);
    if (usesOAuth) {
      const popup = window.open('', 'imail-oauth', 'popup,width=560,height=720,menubar=no,toolbar=no');
      if (!popup) { setError('浏览器阻止了登录窗口，请允许弹出窗口后重试'); setBusy(false); return; }
      popup.document.write('<title>iMail</title><p style="font-family:system-ui;padding:32px">正在打开安全登录…</p>');
      popupRef.current = popup;
      try {
        const result = await api<{ authorizationUrl: string }>('/api/oauth/start', { method: 'POST', body: JSON.stringify({ provider, displayName: form.get('displayName') || undefined, group: form.get('group'), color: '#168f78' }) });
        popup.location.replace(result.authorizationUrl);
      } catch (value) {
        popup.close(); popupRef.current = null; setBusy(false);
        setError(value instanceof Error ? value.message : '无法开始 OAuth 登录');
      }
      return;
    }
    const body: Record<string, unknown> = { provider, email: form.get('email'), displayName: form.get('displayName'), group: form.get('group'), password: form.get('password'), color: '#168f78' };
    if (provider === 'custom') body.settings = { imapHost: form.get('imapHost'), imapPort: Number(form.get('imapPort')), imapSecure: true, smtpHost: form.get('smtpHost'), smtpPort: Number(form.get('smtpPort')), smtpSecure: Number(form.get('smtpPort')) === 465 };
    try { await api('/api/accounts', { method: 'POST', body: JSON.stringify(body) }); await onAdded(); }
    catch (value) { setError(value instanceof Error ? value.message : '连接失败'); }
    finally { setBusy(false); }
  }
  return <Overlay onClose={onClose} wide><form className="account-modal" onSubmit={submit}>
    <div className="modal-header"><div><span>连接新的收件箱</span><h2>添加邮箱</h2><p>优先使用服务商安全登录，iMail 不会接触你的网页登录密码。</p></div><button type="button" onClick={onClose}><X size={21} /></button></div>
    <div className="provider-grid">{providers.map((item) => <button type="button" key={item.id} className={provider === item.id ? 'selected' : ''} onClick={() => { setProvider(item.id); setManualMode(false); setError(''); }}><i className={`provider-mark provider-${item.id}`}>{item.mark}</i><span>{item.name}</span>{item.oauthKey && <small className="oauth-chip">OAuth</small>}{provider === item.id && <Check size={15} weight="bold" />}</button>)}</div>
    {usesOAuth ? <>
      <div className="oauth-panel">
        <div className={`oauth-status ${oauthStatus?.configured ? 'ready' : 'setup'}`}><Key size={21} weight="duotone" /><span><strong>{providerLabel[provider]} 安全登录</strong><small>{oauthStatus?.configured ? 'OAuth 已配置。登录将在服务商官方页面完成，并自动安全刷新授权。' : oauthStatus?.configurationHint || '正在读取 OAuth 配置…'}</small></span></div>
        <div className="form-grid oauth-profile"><label><span>显示名称（可选）</span><input name="displayName" placeholder="默认使用账户名称" /></label><label><span>加入分组</span><select name="group" defaultValue="工作"><option>工作</option><option>个人</option><option>对外支持</option><option>开发测试</option><option>同学联系</option></select></label></div>
        {provider === 'yahoo' && <div className="oauth-review"><WarningCircle size={17} /><span>Yahoo 的 mail-r/mail-w 权限只对审核通过的应用开放。</span></div>}
      </div>
      <button type="button" className="manual-switch" onClick={() => setManualMode(true)}>无法使用 OAuth？改用应用专用密码</button>
    </> : <>
      <div className="form-grid"><label><span>邮箱地址</span><input name="email" type="email" placeholder="name@example.com" required /></label><label><span>显示名称</span><input name="displayName" placeholder="例如：工作邮箱" required /></label><label><span>分组</span><select name="group" defaultValue="工作"><option>工作</option><option>个人</option><option>对外支持</option><option>开发测试</option><option>同学联系</option></select></label><label><span>应用专用密码 / 授权码</span><input name="password" type="password" placeholder="不会以明文保存" required /></label></div>
      {provider !== 'custom' && <div className="provider-tip"><Key size={19} /><span><strong>{providerLabel[provider]} 安全提示</strong><small>{provider === 'qq' ? 'QQ 邮箱尚未公开第三方邮件 OAuth，请在邮箱设置中生成授权码。' : provider === 'icloud' ? 'Apple 尚未公开通用跨平台 iCloud Mail OAuth 接入，请使用应用专用密码。' : '请使用应用专用密码，不要填写网页登录密码。'}</small></span>{selectedProvider.helpUrl && <a href={selectedProvider.helpUrl} target="_blank" rel="noreferrer">打开授权页面 <ArrowRight size={14} /></a>}</div>}
      {selectedProvider.oauthKey && <button type="button" className="manual-switch" onClick={() => setManualMode(false)}>返回 {providerLabel[provider]} OAuth 安全登录</button>}
    </>}
    {provider === 'custom' && <div className="advanced-settings"><button type="button" onClick={() => setAdvanced(!advanced)}><Gear size={17} />IMAP / SMTP 设置<CaretDown size={15} /></button>{(advanced || provider === 'custom') && <div className="form-grid"><label><span>IMAP 主机</span><input name="imapHost" placeholder="imap.example.com" required /></label><label><span>IMAP 端口</span><input name="imapPort" type="number" defaultValue="993" required /></label><label><span>SMTP 主机</span><input name="smtpHost" placeholder="smtp.example.com" required /></label><label><span>SMTP 端口</span><input name="smtpPort" type="number" defaultValue="465" required /></label></div>}</div>}
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
    <div className="modal-footer"><button type="button" onClick={onClose}>取消</button><Button appearance="primary" type="submit" disabled={busy || (usesOAuth && !oauthStatus?.configured)}>{busy ? (usesOAuth ? '等待授权…' : '正在验证连接…') : usesOAuth ? `使用 ${providerLabel[provider]} 登录` : '验证并添加'}</Button></div>
  </form></Overlay>;
}

function AccountSettingsModal({ accounts, onClose, onReload, setNotice }: { accounts: Account[]; onClose: () => void; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState('');
  const popupRef = useRef<Window | null>(null);

  useEffect(() => {
    const receive = (event: MessageEvent) => {
      if (event.source !== popupRef.current || event.data?.source !== 'imail-oauth') return;
      popupRef.current = null;
      setBusyId(null);
      if (event.data.success) {
        void onReload().then(() => setNotice({ kind: 'success', text: '邮箱授权已更新' }));
      } else setError(event.data.message || '重新授权未完成');
    };
    window.addEventListener('message', receive);
    const timer = window.setInterval(() => {
      if (popupRef.current?.closed) { popupRef.current = null; setBusyId(null); }
    }, 700);
    return () => { window.removeEventListener('message', receive); window.clearInterval(timer); };
  }, [onReload, setNotice]);

  async function reconnect(account: Account) {
    setError('');
    const popup = window.open('', 'imail-oauth', 'popup,width=560,height=720,menubar=no,toolbar=no');
    if (!popup) { setError('浏览器阻止了登录窗口，请允许弹出窗口后重试'); return; }
    popup.document.write('<title>iMail</title><p style="font-family:system-ui;padding:32px">正在打开安全登录…</p>');
    popupRef.current = popup;
    setBusyId(account.id);
    try {
      const result = await api<{ authorizationUrl: string }>(`/api/accounts/${account.id}/oauth/reconnect`, { method: 'POST' });
      popup.location.replace(result.authorizationUrl);
    } catch (value) {
      popup.close(); popupRef.current = null; setBusyId(null);
      setError(value instanceof Error ? value.message : '无法开始重新授权');
    }
  }

  async function remove(account: Account) {
    if (!window.confirm(`确定从 iMail 移除 ${account.email}？本地邮件缓存也会一并删除。`)) return;
    setBusyId(account.id); setError('');
    try {
      await api(`/api/accounts/${account.id}`, { method: 'DELETE' });
      await onReload();
      setNotice({ kind: 'success', text: `${account.displayName} 已从本机移除` });
    } catch (value) { setError(value instanceof Error ? value.message : '移除失败'); }
    finally { setBusyId(null); }
  }

  return <Overlay onClose={onClose}><section className="account-settings-modal">
    <div className="modal-header"><div><span>连接与授权</span><h2>邮箱设置</h2><p>查看连接状态，更新 OAuth 授权或移除本地账户。</p></div><button type="button" onClick={onClose}><X size={21} /></button></div>
    {accounts.length === 0 ? <div className="settings-empty"><Envelope size={38} weight="duotone" /><h3>还没有真实邮箱</h3><p>关闭设置后，点击左侧的加号接入第一个邮箱。</p></div> : <div className="settings-account-list">
      {accounts.map((account) => <article key={account.id} className="settings-account">
        <i style={{ background: account.color }}>{initials(account.displayName)}</i>
        <span><strong>{account.displayName}</strong><small>{account.email}</small><em className={`connection-${account.status}`}>{account.status === 'connected' ? '连接正常' : account.status === 'syncing' ? '正在同步' : account.lastError || '连接异常'}</em></span>
        <div><small>{account.authMethod === 'oauth2' ? 'OAuth 2.0' : '授权码 / 专用密码'}</small>{account.authMethod === 'oauth2' && <button type="button" className="reconnect-account" disabled={busyId === account.id} onClick={() => void reconnect(account)}><ArrowClockwise size={15} />{busyId === account.id ? '等待登录' : '重新授权'}</button>}<button type="button" className="remove-account" disabled={busyId === account.id} onClick={() => void remove(account)}><Trash size={15} />移除</button></div>
      </article>)}
    </div>}
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
    <div className="modal-footer"><Button appearance="primary" onClick={onClose}>完成</Button></div>
  </section></Overlay>;
}

function ComposeModal({ accounts, reply, onClose, onSent }: { accounts: Account[]; reply?: Message; onClose: () => void; onSent: () => void }) {
  const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); const form = new FormData(event.currentTarget); setBusy(true); setError('');
    try { await api('/api/send', { method: 'POST', body: JSON.stringify({ accountId: form.get('accountId'), to: String(form.get('to')).split(',').map((item) => item.trim()), subject: form.get('subject'), text: form.get('text') }) }); onSent(); }
    catch (value) { setError(value instanceof Error ? value.message : '发送失败'); } finally { setBusy(false); }
  }
  return <Overlay onClose={onClose}><form className="compose-modal" onSubmit={submit}><div className="modal-header compact"><div><span>新邮件</span><h2>{reply ? '回复邮件' : '写邮件'}</h2></div><button type="button" onClick={onClose}><X size={21} /></button></div>
    {accounts.length === 0 ? <div className="compose-empty"><WarningCircle size={30} /><h3>先接入一个真实邮箱</h3><p>接入邮箱后即可发送邮件。</p></div> : <><label className="compose-row"><span>发件人</span><select name="accountId" defaultValue={reply?.accountId}>{accounts.map((account) => <option key={account.id} value={account.id}>{account.displayName} · {account.email}</option>)}</select></label><label className="compose-row"><span>收件人</span><input name="to" type="email" defaultValue={reply?.from.address} required /></label><label className="compose-row"><span>主题</span><input name="subject" defaultValue={reply ? `Re: ${reply.subject}` : ''} required /></label><textarea name="text" className="compose-body" defaultValue={reply ? `\n\n----- 原邮件 -----\n${reply.text ?? ''}` : ''} placeholder="写下邮件内容…" required />{error && <div className="inline-error">{error}</div>}<div className="modal-footer"><button type="button" onClick={onClose}>存为草稿</button><Button appearance="primary" icon={<PaperPlaneTilt size={17} />} type="submit" disabled={busy}>{busy ? '发送中…' : '发送邮件'}</Button></div></>}
  </form></Overlay>;
}

function TokenWorkspace({ accounts, tokens, onCreate, onReload, setNotice }: { accounts: Account[]; tokens: DeveloperToken[]; onCreate: () => void; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  async function revoke(id: string) { await api(`/api/developer-tokens/${id}`, { method: 'DELETE' }); await onReload(); setNotice({ kind: 'success', text: 'Token 已撤销' }); }
  return <section className="token-workspace"><header><div><span>本地开发能力</span><h1>邮件网关</h1><p>让本地项目用一个短期 Token 安全读取或发送邮件，无需重复配置每个邮箱的 IMAP。</p></div><Button appearance="primary" icon={<Plus size={17} />} onClick={onCreate} disabled={accounts.length === 0}>创建临时 Token</Button></header>
    <div className="endpoint-strip"><Code size={21} /><span><small>开发 API 地址</small><code>http://127.0.0.1:8787/api/dev/v1</code></span><button onClick={() => void navigator.clipboard.writeText('http://127.0.0.1:8787/api/dev/v1')}><Copy size={17} />复制</button></div>
    <div className="token-columns"><div className="token-list"><div className="token-title"><h2>有效 Token</h2><span>{tokens.filter((token) => token.expiresAt > new Date().toISOString()).length} 个正在生效</span></div>{accounts.length === 0 ? <div className="token-empty"><Key size={38} weight="duotone" /><h3>接入邮箱后即可创建</h3><p>Token 只会访问你明确选择的邮箱和权限。</p></div> : tokens.length === 0 ? <div className="token-empty"><Key size={38} weight="duotone" /><h3>还没有临时 Token</h3><p>创建一个给本地应用使用，原始值只显示一次。</p><button onClick={onCreate}>创建第一个 Token</button></div> : tokens.map((token) => <article className="token-item" key={token.id}><div className="token-icon"><Key size={20} /></div><div><strong>{token.name}</strong><code>{token.prefix}••••••••••••</code><span>{token.scopes.map((scope) => scope.replace('messages:', '')).join(' · ')} · {token.accountIds.length} 个邮箱</span></div><div className="token-time"><small>到期时间</small><span>{new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(token.expiresAt))}</span></div><button className="revoke" onClick={() => void revoke(token.id)}>撤销</button></article>)}</div>
      <aside className="quickstart"><div className="quickstart-title"><AddressBook size={21} /><div><strong>快速调用</strong><span>读取最新 10 封邮件</span></div></div><pre><code><span className="code-muted">curl</span> http://127.0.0.1:8787/api/dev/v1/messages?limit=10 \\{`\n`}  -H <span className="code-string">&quot;Authorization: Bearer imail_xxx&quot;</span></code></pre><div className="security-note"><WarningCircle size={18} /><p><strong>最小权限原则</strong><span>只选择当前测试真正需要的邮箱。Token 到期后会自动失效。</span></p></div><a href="/README.md" target="_blank">查看完整 API 文档 <ArrowRight size={15} /></a></aside></div>
  </section>;
}

function CreateTokenModal({ accounts, onClose, onCreated }: { accounts: Account[]; onClose: () => void; onCreated: () => Promise<void> }) {
  const [raw, setRaw] = useState(''); const [busy, setBusy] = useState(false); const [copied, setCopied] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) { event.preventDefault(); setBusy(true); const form = new FormData(event.currentTarget); const accountIds = form.getAll('accountIds'); const scopes = form.getAll('scopes'); try { const result = await api<{ token: string }>('/api/developer-tokens', { method: 'POST', body: JSON.stringify({ name: form.get('name'), accountIds, scopes, ttlSeconds: Number(form.get('ttlSeconds')) }) }); setRaw(result.token); await onCreated(); } finally { setBusy(false); } }
  return <Overlay onClose={onClose}>{raw ? <div className="token-created"><div className="success-orbit"><Check size={28} weight="bold" /></div><h2>Token 已创建</h2><p>请现在复制并保存，关闭后无法再次查看完整值。</p><div className="raw-token"><code>{raw}</code><button onClick={async () => { await navigator.clipboard.writeText(raw); setCopied(true); }}><Copy size={17} />{copied ? '已复制' : '复制'}</button></div><Button appearance="primary" onClick={onClose}>完成</Button></div> : <form className="token-modal" onSubmit={submit}><div className="modal-header"><div><span>开发者网关</span><h2>创建临时 Token</h2><p>控制可访问邮箱、能力和有效时间。</p></div><button type="button" onClick={onClose}><X size={21} /></button></div><label><span>用途名称</span><input name="name" defaultValue="本地开发测试" required /></label><fieldset><legend>允许访问的邮箱</legend>{accounts.map((account) => <label className="check-row" key={account.id}><input name="accountIds" type="checkbox" value={account.id} defaultChecked /><i style={{ background: account.color }}>{initials(account.displayName)}</i><span><strong>{account.displayName}</strong><small>{account.email}</small></span><Check size={15} /></label>)}</fieldset><fieldset><legend>权限范围</legend><label className="scope-row"><input name="scopes" type="checkbox" value="messages:read" defaultChecked /><span><strong>读取邮件</strong><small>获取正文、发件人与附件元数据</small></span></label><label className="scope-row"><input name="scopes" type="checkbox" value="messages:send" /><span><strong>发送邮件</strong><small>通过选定邮箱发送新邮件</small></span></label><label className="scope-row"><input name="scopes" type="checkbox" value="accounts:read" /><span><strong>读取账户</strong><small>获取邮箱列表和连接状态</small></span></label></fieldset><label><span>有效时间</span><select name="ttlSeconds" defaultValue="3600"><option value="1800">30 分钟</option><option value="3600">1 小时</option><option value="21600">6 小时</option><option value="86400">24 小时</option><option value="604800">7 天</option></select></label><div className="modal-footer"><button type="button" onClick={onClose}>取消</button><Button appearance="primary" type="submit" disabled={busy}>{busy ? '创建中…' : '创建 Token'}</Button></div></form>}</Overlay>;
}

export default App;
