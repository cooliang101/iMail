import { useCallback, type Dispatch, type SetStateAction } from 'preact/compat';
import type { Notice } from '../../app-model';
import { api, workItemsFromResponse } from '../../services';
import type { MailWorkItemView, MailWorkStatus } from '../../types';

type Options = {
  workItems: MailWorkItemView[];
  selectedId: string | null;
  setSelectedId: Dispatch<SetStateAction<string | null>>;
  setWorkItems: Dispatch<SetStateAction<MailWorkItemView[]>>;
  setNotice: Dispatch<SetStateAction<Notice>>;
};

export function useWorkQueueActions({ workItems, selectedId, setSelectedId, setWorkItems, setNotice }: Options) {
  const refresh = useCallback(async () => {
    const result = await api<unknown>('/api/mail-work-items');
    setWorkItems(workItemsFromResponse(result));
  }, [setWorkItems]);

  const setWorkItem = useCallback(async (messageId: string, status: MailWorkStatus = 'needsReply') => {
    const existing = workItems.find(({ item }) => item.messageId === messageId)?.item;
    try {
      await api(`/api/messages/${encodeURIComponent(messageId)}/work-item`, {
        method: 'PUT',
        body: JSON.stringify({ status, dueAt: existing?.dueAt, note: existing?.note ?? '' }),
      });
      await refresh();
      setNotice({ kind: 'success', text: existing ? '处理状态已更新' : '邮件已加入处理队列' });
    } catch (error) {
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '处理队列更新失败' });
    }
  }, [refresh, setNotice, workItems]);

  const completeWorkItem = useCallback(async (messageId: string) => {
    try {
      await api(`/api/messages/${encodeURIComponent(messageId)}/work-item`, { method: 'DELETE' });
      await refresh();
      if (selectedId === messageId) setSelectedId(null);
      setNotice({ kind: 'success', text: '处理项目已完成' });
    } catch (error) {
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '处理项目完成失败' });
    }
  }, [refresh, selectedId, setNotice, setSelectedId]);

  return { setWorkItem, completeWorkItem };
}
