export function relativeTime(value: string) {
  const diff = Date.now() - new Date(value).getTime();
  if (diff < 60 * 60_000) return `${Math.max(1, Math.floor(diff / 60_000))} 分钟前`;
  if (diff < 24 * 60 * 60_000) return `${Math.floor(diff / 3600_000)} 小时前`;
  if (diff < 48 * 60 * 60_000) return '昨天';
  return new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric' }).format(new Date(value));
}
