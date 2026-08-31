import type { SearchFilters } from '../../app-model';

export function filtersFromQuery(query: string): SearchFilters {
  const params = new URLSearchParams(query);
  const filters: SearchFilters = params.has('filters') ? JSON.parse(params.get('filters')!) as SearchFilters : {};
  if (params.has('accountId')) filters.accountIds = [params.get('accountId')!];
  for (const field of ['group', 'q', 'sender', 'recipient', 'mailboxRole', 'mailbox', 'mailboxName'] as const) {
    if (params.has(field)) filters[field] = params.get(field);
  }
  for (const field of ['unread', 'flagged', 'hasAttachments', 'snoozed'] as const) {
    if (params.has(field)) filters[field] = params.get(field) === 'true';
  }
  if (params.has('label')) filters.labels = [params.get('label')!];
  if (!params.has('filters') && filters.mailboxRole === 'inbox' && filters.snoozed !== true) filters.snoozed = false;
  return filters;
}

export function localDateInput(instant?: string | null) {
  if (!instant) return '';
  const date = new Date(instant);
  return new Date(date.getTime() - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
}

export function validateFilters(filters: SearchFilters): string | null {
  if (filters.since && filters.before && new Date(filters.since) >= new Date(filters.before)) return '结束时间必须晚于开始时间';
  for (const address of [filters.sender, filters.recipient]) {
    if (address && (!address.includes('@') || /\s/.test(address))) return '发件人和收件人需要完整邮箱地址';
  }
  return null;
}
