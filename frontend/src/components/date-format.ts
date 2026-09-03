export function parseValidDate(value: string | null | undefined) {
  if (!value) return undefined;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? undefined : date;
}

export function formatDate(
  value: string | null | undefined,
  options: Intl.DateTimeFormatOptions,
  locale = 'zh-CN',
  fallback = '—',
) {
  const date = parseValidDate(value);
  return date ? new Intl.DateTimeFormat(locale, options).format(date) : fallback;
}

export function relativeTime(value: string | null | undefined, locale = 'zh-CN') {
  const date = parseValidDate(value);
  if (!date) return locale === 'en-US' ? 'Unknown time' : '时间未知';
  const diff = Date.now() - date.getTime();
  if (diff < 0) return new Intl.DateTimeFormat(locale, { month: 'numeric', day: 'numeric' }).format(date);
  if (locale === 'en-US') {
    if (diff < 60 * 60_000) return `${Math.max(1, Math.floor(diff / 60_000))}m ago`;
    if (diff < 24 * 60 * 60_000) return `${Math.floor(diff / 3600_000)}h ago`;
    if (diff < 48 * 60 * 60_000) return 'Yesterday';
  } else {
    if (diff < 60 * 60_000) return `${Math.max(1, Math.floor(diff / 60_000))} 分钟前`;
    if (diff < 24 * 60 * 60_000) return `${Math.floor(diff / 3600_000)} 小时前`;
    if (diff < 48 * 60 * 60_000) return '昨天';
  }
  return new Intl.DateTimeFormat(locale, { month: 'numeric', day: 'numeric' }).format(date);
}
