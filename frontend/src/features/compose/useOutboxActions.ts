import { useCallback, type Dispatch, type SetStateAction } from 'preact/compat';
import type { Notice } from '../../app-model';
import { api } from '../../services';

type Options = {
  reloadAll: () => Promise<void>;
  reloadOutbox: () => Promise<void>;
  setNotice: Dispatch<SetStateAction<Notice>>;
};

export function useOutboxActions({ reloadAll, reloadOutbox, setNotice }: Options) {
  const run = useCallback(async (path: string, options: RequestInit, success: string, fallback: string) => {
    try {
      await api(path, options);
      await reloadAll();
      setNotice({ kind: 'success', text: success });
    } catch (error) {
      void reloadOutbox();
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : fallback });
    }
  }, [reloadAll, reloadOutbox, setNotice]);

  const cancelOutboxItem = useCallback((id: string) => run(
    `/api/outbox/${id}`,
    { method: 'DELETE' },
    '定时任务已取消，邮件已返回草稿',
    '定时任务取消失败',
  ), [run]);

  const retryOutboxItem = useCallback((id: string) => run(
    `/api/outbox/${id}/retry`,
    { method: 'POST' },
    '邮件已重新加入发送队列',
    '邮件重新发送失败',
  ), [run]);

  const resolveOutboxItem = useCallback((id: string, resolution: 'sent' | 'notSent') => run(
    `/api/outbox/${id}/resolve`,
    { method: 'POST', body: JSON.stringify({ resolution }) },
    resolution === 'sent' ? '邮件已标记为人工核对完成' : '邮件已返回草稿',
    '人工核对结果保存失败',
  ), [run]);

  return { cancelOutboxItem, retryOutboxItem, resolveOutboxItem };
}
