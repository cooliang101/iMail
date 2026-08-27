import { useCallback, useEffect, useMemo, useRef, useState, type Dispatch, type SetStateAction } from 'preact/compat';
import { api, desktopLog, describeDesktopLogValue, subscribeSyncEvents } from '../../services';
import { buildMessageQuery } from '../../app/selectors';
import type { Account, Contact, Message } from '../../types';
import type { AppView, MessageStats, Notice, ParticipantFilters, WorkspaceFolder } from '../../app-model';
import { appendMessagePage, applyMessageChanges, applyMessageStatsChanges, cacheMessageBody, messageTotalDelta, type MessageChange } from './message-cache';
import type { MailListFilter } from './MessagePane';

type MessagePage = { messages: Message[]; total: number; nextOffset: number; nextCursor?: string; hasMore: boolean };

type Options = {
  accounts: Account[];
  view: AppView;
  accountFilter: string;
  groupFilter: string | null;
  search: string;
  participantFilters: ParticipantFilters;
  mailFilter: MailListFilter;
  activeLabel: string | null;
  activeMailbox: WorkspaceFolder | null;
  selectedId: string | null;
  setSelectedId: Dispatch<SetStateAction<string | null>>;
  setContacts: Dispatch<SetStateAction<Contact[]>>;
  setNotice: Dispatch<SetStateAction<Notice>>;
  isMessageActionActive?: (messageId: string) => boolean;
};

export function useMessageCollection(options: Options) {
  const { accounts, view, accountFilter, groupFilter, search, participantFilters, mailFilter, activeLabel, activeMailbox, selectedId, setSelectedId, setContacts, setNotice, isMessageActionActive } = options;
  const [messages, setMessages] = useState<Message[]>([]);
  const [messageTotal, setMessageTotal] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [nextCursor, setNextCursor] = useState<string | undefined>();
  const [loading, setLoading] = useState(false);
  const [ready, setReady] = useState(false);
  const [revision, setRevision] = useState(0);
  const [stats, setStats] = useState<MessageStats>({ total: 0, unread: 0, byAccount: [], byGroup: [] });
  const query = useMemo(() => buildMessageQuery({ accountFilter, groupFilter, search, view, mailFilter, activeLabel, activeMailbox, participantFilters }), [accountFilter, groupFilter, search, view, mailFilter, activeLabel, activeMailbox, participantFilters]);
  const queryRef = useRef(query);
  const accountsRef = useRef(accounts);
  const implicitSelectedIdRef = useRef<string | null>(null);
  const recentBodyIdsRef = useRef<string[]>([]);
  const actionActiveRef = useRef(isMessageActionActive);
  const selectedIdRef = useRef(selectedId);

  useEffect(() => { queryRef.current = query; }, [query]);
  useEffect(() => { accountsRef.current = accounts; }, [accounts]);
  useEffect(() => { actionActiveRef.current = isMessageActionActive; }, [isMessageActionActive]);
  useEffect(() => { selectedIdRef.current = selectedId; }, [selectedId]);

  useEffect(() => subscribeSyncEvents(['sync.completed'], (event: MessageEvent) => {
    try {
      const changes = ((JSON.parse(event.data) as { payload?: { messageChanges?: MessageChange[] } }).payload?.messageChanges ?? [])
        .filter((change) => !actionActiveRef.current?.(change.after?.id ?? change.before?.id ?? ''));
      if (changes.length === 0) return;
      const currentQuery = queryRef.current;
      const currentAccounts = accountsRef.current;
      setMessages((current) => applyMessageChanges(current, changes, currentQuery, currentAccounts));
      setMessageTotal((current) => Math.max(0, current + messageTotalDelta(changes, currentQuery, currentAccounts)));
      setStats((current) => applyMessageStatsChanges(current, changes, currentAccounts));
      void api<{ contacts: Contact[] }>('/api/contacts').then((result) => setContacts(result.contacts))
        .catch((error) => desktopLog('warn', 'contacts.refresh_failed', describeDesktopLogValue(error)));
    } catch (error) {
      void desktopLog('warn', 'sync.event_invalid', describeDesktopLogValue(error));
      // Optional malformed event payloads must not interrupt the mailbox view.
    }
  }), [setContacts]);

  useEffect(() => {
    if (view === 'contacts' || view === 'tokens') { setLoading(false); setReady(true); return; }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      setLoading(true);
      void api<MessagePage>(`/api/messages?${query}&limit=60&offset=0`).then((result) => {
        if (cancelled) return;
        implicitSelectedIdRef.current = result.messages[0]?.id ?? null;
        setMessages(result.messages);
        setMessageTotal(result.total);
        setHasMore(result.hasMore);
        setNextCursor(result.nextCursor);
        setSelectedId(result.messages.some((message) => message.id === selectedIdRef.current) ? selectedIdRef.current : null);
      }).catch((error) => {
        if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮件缓存加载失败' });
      }).finally(() => {
        if (!cancelled) { setLoading(false); setReady(true); }
      });
    }, search.trim() ? 220 : 0);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [query, revision, search, setNotice, setSelectedId, view]);

  const selected = messages.find((message) => message.id === (selectedId ?? implicitSelectedIdRef.current)) ?? messages[0];
  useEffect(() => {
    if (view === 'contacts' || view === 'tokens' || !selected || selected.text !== undefined) return;
    let cancelled = false;
    void api<{ message: Message }>(`/api/messages/${selected.id}`).then(({ message }) => {
      if (!cancelled) setMessages((current) => {
        const cached = cacheMessageBody(current, message, recentBodyIdsRef.current);
        recentBodyIdsRef.current = cached.recentBodyIds;
        return cached.messages;
      });
    }).catch((error) => {
      if (!cancelled) setNotice({ kind: 'error', text: error instanceof Error ? error.message : '邮件正文加载失败' });
    });
    return () => { cancelled = true; };
  }, [selected, setNotice, view]);

  const loadMore = useCallback(async () => {
    if (loading || !hasMore) return;
    const queryAtStart = query;
    setLoading(true);
    try {
      const cursorParameter = nextCursor ? `&cursor=${encodeURIComponent(nextCursor)}` : '';
      const result = await api<MessagePage>(`/api/messages?${queryAtStart}&limit=60${cursorParameter}`);
      if (queryRef.current !== queryAtStart) return;
      setMessages((current) => appendMessagePage(current, result.messages));
      setMessageTotal(result.total);
      setHasMore(result.hasMore);
      setNextCursor(result.nextCursor);
    } catch (error) {
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '加载更多邮件失败' });
    } finally {
      setLoading(false);
    }
  }, [hasMore, loading, nextCursor, query, setNotice]);

  return {
    messages,
    setMessages,
    messageTotal,
    setMessageTotal,
    hasMore,
    loading,
    ready,
    revision,
    setRevision,
    stats,
    setStats,
    selected,
    loadMore,
  };
}
