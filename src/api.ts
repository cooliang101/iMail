export async function api<T>(path: string, options?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...options,
    credentials: 'include',
    headers: { 'Content-Type': 'application/json', ...options?.headers },
  });
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: response.statusText }));
    const error = new Error(body.error ?? '请求失败') as Error & { status?: number };
    error.status = response.status;
    if (response.status === 401 && !path.startsWith('/api/auth/')) window.dispatchEvent(new Event('imail:unauthorized'));
    throw error;
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}
