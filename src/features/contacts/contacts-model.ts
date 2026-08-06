import type { Contact } from '../../types';

export function filterContacts(contacts: Contact[], queryValue: string) {
  const query = queryValue.trim().toLocaleLowerCase();
  if (!query) return contacts;
  return contacts.filter((contact) => `${contact.name}\n${contact.address}`.toLocaleLowerCase().includes(query));
}

export function contactAccent(address: string) {
  const colors = ['#2f8f7a', '#4479b7', '#8a62aa', '#c47a32', '#b85f72', '#487f92'];
  let hash = 0;
  for (const character of address.toLocaleLowerCase()) hash = ((hash << 5) - hash + character.charCodeAt(0)) | 0;
  return colors[Math.abs(hash) % colors.length];
}

export function contactRecency(value: string, now = new Date()) {
  const date = new Date(value);
  const elapsedDays = Math.max(0, Math.floor((now.getTime() - date.getTime()) / 86_400_000));
  if (elapsedDays === 0) return '今天联系';
  if (elapsedDays === 1) return '昨天联系';
  if (elapsedDays < 30) return `${elapsedDays} 天前联系`;
  return new Intl.DateTimeFormat('zh-CN', { year: date.getFullYear() === now.getFullYear() ? undefined : 'numeric', month: 'short', day: 'numeric' }).format(date);
}
