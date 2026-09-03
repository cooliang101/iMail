import { useCallback, useEffect, useMemo, useRef, useState, type Dispatch, type SetStateAction } from 'preact/compat';
import { api, contactsFromResponse, desktopLog, describeDesktopLogValue, messageFromResponse, subscribeSyncEvents } from '../../services';
import { buildMessageQuery } from './message-query';
import type { Account, Contact, Message } from '../../types';
import type { AppView, MessageStats, Notice, ParticipantFilters, SearchFilters, WorkspaceFolder } from '../../app-model';
import { appendMessagePage, applyMessageChanges, applyMessageStatsChanges, cacheMessageBody, messageTotalDelta, type MessageChange } from './message-cache';
import type { MailListFilter } from './MessagePane';
import { messagePageFromResponse } from './mail-response';
const MESSAGE_PAGE_SIZE = 60;

type Options = {
  searchFilters?: SearchFilters | null;
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
  const query = useMemo(() => buildMessageQuery({ accountFilter, groupFilter, search, view, mailFilter, activeLabel, activeMailbox, participantFilters, searchFilters: options.searchFilters }), [accountFilter, groupFilter, search, view, mailFilter, activeLabel, activeMailbox, participantFilters, options.searchFilters]);
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
      if (new URLSearchParams(queryRef.current).has('filters')) {
        // Sync summaries omit bodies. Re-query the authoritative index, never infer
        // advanced matches or counts from incomplete client-side messages.
        setRevision((value) => value + 1);
        void api<MessageStats>('/api/message-stats').then(setStats).catch(() => {});
        void api<unknown>('/api/contacts').then((result) => setContacts(contactsFromResponse(result)))
          .catch((error) => desktopLog('warn', 'contacts.refresh_failed', describeDesktopLogValue(error)));
        return;
      }
      if (changes.length === 0) return;
      const currentQuery = queryRef.current;
      const currentAccounts = accountsRef.current;
      setMessages((current) => applyMessageChanges(current, changes, currentQuery, currentAccounts));
      setMessageTotal((current) => Math.max(0, current + messageTotalDelta(changes, currentQuery, currentAccounts)));
      setStats((current) => applyMessageStatsChanges(current, changes, currentAccounts));
      void api<unknown>('/api/contacts').then((result) => setContacts(contactsFromResponse(result)))
        .catch((error) => desktopLog('warn', 'contacts.refresh_failed', describeDesktopLogValue(error)));
    } catch (error) {
      void desktopLog('warn', 'sync.event_invalid', describeDesktopLogValue(error));
      // Optional malformed event payloads must not interrupt the mailbox view.
    }
  }), [setContacts]);

  useEffect(() => {
    if (view === 'contacts' || view === 'tokens' || view === 'workQueue') { setLoading(false); setReady(true); return; }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      setLoading(true);
      void api<unknown>(`/api/messages?${query}&limit=${MESSAGE_PAGE_SIZE}&offset=0`).then(messagePageFromResponse).then((result) => {
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
    void api<unknown>(`/api/messages/${selected.id}`).then(messageFromResponse).then((message) => {
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
      const result = messagePageFromResponse(await api<unknown>(`/api/messages?${queryAtStart}&limit=${MESSAGE_PAGE_SIZE}${cursorParameter}`));
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
