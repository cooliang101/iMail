import { useCallback, useEffect, useState, type Dispatch, type SetStateAction } from 'preact/compat';
import type { MessageStats, Notice } from '../../app-model';
import { api } from '../../services';
import type { Account, Message } from '../../types';
import { MESSAGE_MOVE_UNDO_MS, remainingMessageMoveUndoMs } from './constants';
import type { MailListFilter } from './MessagePane';
import { applyOptimisticMessageMutation, MessageActionCoordinator, rollbackOptimisticMessageMutation } from './message-actions';

type PendingMove = { message: Message; index: number; nextId: string | null; destination: 'archive' | 'trash'; unreadDelta: number; deadlineAt: number };

type Options = {
  accounts: Account[];
  messages: Message[];
  selected?: Message;
  view: string;
  mailFilter: MailListFilter;
  coordinator: MessageActionCoordinator;
  setMessages: Dispatch<SetStateAction<Message[]>>;
  setMessageTotal: Dispatch<SetStateAction<number>>;
  setMessageStats: Dispatch<SetStateAction<MessageStats>>;
  setMessageRevision: Dispatch<SetStateAction<number>>;
  setSelectedId: Dispatch<SetStateAction<string | null>>;
  setNotice: Dispatch<SetStateAction<Notice>>;
};

export function useMessageActions({ accounts, messages, selected, view, mailFilter, coordinator, setMessages, setMessageTotal, setMessageStats, setMessageRevision, setSelectedId, setNotice }: Options) {
  const [busy, setBusy] = useState(false);
  const [pendingMove, setPendingMove] = useState<PendingMove | null>(null);

  const adjustStats = useCallback((accountId: string, totalDelta: number, unreadDelta: number) => {
    const account = accounts.find((item) => item.id === accountId);
    setMessageStats((current) => ({
      ...current,
      total: Math.max(0, current.total + totalDelta),
      unread: Math.max(0, current.unread + unreadDelta),
      byAccount: current.byAccount.map((item) => item.accountId === accountId ? { ...item, total: Math.max(0, item.total + totalDelta), unread: Math.max(0, item.unread + unreadDelta) } : item),
      byGroup: current.byGroup.map((item) => item.group === account?.group ? { ...item, total: Math.max(0, item.total + totalDelta), unread: Math.max(0, item.unread + unreadDelta) } : item),
    }));
  }, [accounts, setMessageStats]);

  const setMessageUnread = useCallback(async (message: Message, unread: boolean) => {
    if (message.unread === unread || coordinator.isActive(message.id)) return;
    await coordinator.run(message.id, async () => {
      setBusy(true);
      setMessages((current) => applyOptimisticMessageMutation(current, message.id, { unread }));
      if (mailFilter === 'unread') setMessageTotal((current) => Math.max(0, current + (unread ? 1 : -1)));
      if (message.mailboxRole === 'inbox') adjustStats(message.accountId, 0, unread ? 1 : -1);
      try {
        await api(`/api/messages/${message.id}`, { method: 'PATCH', body: JSON.stringify({ unread }) });
        setNotice({ kind: 'success', text: unread ? '邮件已标记为未读' : '邮件已标记为已读' });
      } catch (error) {
        setMessages((current) => rollbackOptimisticMessageMutation(current, message.id, { unread }, { unread: message.unread }));
        if (mailFilter === 'unread') setMessageTotal((current) => Math.max(0, current + (unread ? -1 : 1)));
        if (message.mailboxRole === 'inbox') adjustStats(message.accountId, 0, unread ? -1 : 1);
        setNotice({ kind: 'error', text: error instanceof Error ? error.message : '已读状态更新失败' });
      } finally {
        setMessageRevision((value) => value + 1);
        setBusy(false);
      }
    });
  }, [adjustStats, coordinator, mailFilter, setMessageRevision, setMessageTotal, setMessages, setNotice]);

  const markSelectedUnread = useCallback(async () => {
    if (selected) await setMessageUnread(selected, true);
  }, [selected, setMessageUnread]);

  const toggleSelectedFlag = useCallback(async (target = selected) => {
    if (!target || coordinator.isActive(target.id)) return;
    const flagged = !target.flagged;
    await coordinator.run(target.id, async () => {
      setBusy(true);
      setMessages((current) => applyOptimisticMessageMutation(current, target.id, { flagged }));
      try {
        await api(`/api/messages/${target.id}`, { method: 'PATCH', body: JSON.stringify({ flagged }) });
        if (view === 'starred' && !flagged) setMessageRevision((value) => value + 1);
      } catch (error) {
        setMessages((current) => rollbackOptimisticMessageMutation(current, target.id, { flagged }, { flagged: target.flagged }));
        setNotice({ kind: 'error', text: error instanceof Error ? error.message : '星标更新失败' });
      } finally {
        setMessageRevision((value) => value + 1);
        setBusy(false);
      }
    });
  }, [coordinator, selected, setMessageRevision, setMessages, setNotice, view]);

  const restorePendingMove = useCallback((move: PendingMove) => {
    const { message, index, unreadDelta } = move;
    setMessages((current) => {
      if (current.some((item) => item.id === message.id)) return current;
      const restored = [...current];
      restored.splice(Math.min(index, restored.length), 0, message);
      return restored;
    });
    setMessageTotal((current) => current + 1);
    if (message.mailboxRole === 'inbox') adjustStats(message.accountId, 1, -unreadDelta);
    setSelectedId(message.id);
    setBusy(false);
  }, [adjustStats, setMessageTotal, setMessages, setSelectedId]);

  const moveSelected = useCallback(async (destination: 'archive' | 'trash', target = selected) => {
    if (!target || busy || !coordinator.begin(target.id)) return;
    const index = messages.findIndex((item) => item.id === target.id);
    const nextId = messages[index + 1]?.id ?? messages[index - 1]?.id ?? null;
    const unreadDelta = target.unread ? -1 : 0;
    setBusy(true);
    setMessages((current) => current.filter((item) => item.id !== target.id));
    setMessageTotal((current) => Math.max(0, current - 1));
    if (target.mailboxRole === 'inbox') adjustStats(target.accountId, -1, unreadDelta);
    setSelectedId(nextId);
    setPendingMove({ message: target, index, nextId, destination, unreadDelta, deadlineAt: Date.now() + MESSAGE_MOVE_UNDO_MS });
  }, [adjustStats, busy, coordinator, messages, selected, setMessageTotal, setMessages, setSelectedId]);

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
        coordinator.end(move.message.id);
        setMessageRevision((value) => value + 1);
        setBusy(false);
      });
    }, remainingMessageMoveUndoMs(pendingMove.deadlineAt));
    return () => window.clearTimeout(timer);
  }, [coordinator, pendingMove, restorePendingMove, setMessageRevision, setNotice]);

  const undoPendingMove = useCallback(() => {
    if (!pendingMove) return;
    setPendingMove(null);
    restorePendingMove(pendingMove);
    coordinator.end(pendingMove.message.id);
    setNotice({ kind: 'success', text: '已撤销邮件移动' });
  }, [coordinator, pendingMove, restorePendingMove, setNotice]);

  return { messageActionBusy: busy, pendingMove, setMessageUnread, markSelectedUnread, toggleSelectedFlag, moveSelected, undoPendingMove };
}
