import { useCallback, useEffect, useRef, useState } from 'preact/compat';
import type { SearchFilters, SmartFolder } from '../../app-model';
import { api, responseObjectArray } from '../../services';

function invalidateGeneration(generation: { current: number }) {
  generation.current += 1;
}

export function useSmartFolders(userId: string) {
  const [folders, setFolders] = useState<SmartFolder[]>([]);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const generation = useRef(0);
  const reload = useCallback(async () => {
    const current = ++generation.current;
    setLoading(true); setError('');
    try {
      const result = await api<unknown>('/api/smart-folders');
      if (current === generation.current) setFolders(responseObjectArray<SmartFolder>(result, 'folders'));
    } catch (cause) { if (current === generation.current) setError(cause instanceof Error ? cause.message : '智能文件夹加载失败'); }
    finally { if (current === generation.current) setLoading(false); }
  }, []);
  useEffect(() => { setFolders([]); void reload(); return () => invalidateGeneration(generation); }, [reload, userId]);
  async function save(name: string, filters: SearchFilters, id?: string) {
    const current = generation.current;
    const { folder } = await api<{ folder: SmartFolder }>(id ? `/api/smart-folders/${encodeURIComponent(id)}` : '/api/smart-folders', { method: id ? 'PUT' : 'POST', body: JSON.stringify({ name, filters }) });
    if (current === generation.current) setFolders((items) => id ? items.map((item) => item.id === id ? folder : item) : [...items, folder]);
    return folder;
  }
  async function remove(id: string) {
    const current = generation.current;
    await api(`/api/smart-folders/${encodeURIComponent(id)}`, { method: 'DELETE' });
    if (current === generation.current) setFolders((items) => items.filter((item) => item.id !== id));
  }
  return { folders, error, loading, reload, save, remove };
}
