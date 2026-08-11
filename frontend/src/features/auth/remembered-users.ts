import type { AppUser } from './auth-context';

export type RememberedUser = Pick<AppUser, 'login' | 'displayName'>;
const rememberedKey = 'imail.remembered-app-users';

export function loadRememberedUsers(): RememberedUser[] {
  try { return JSON.parse(localStorage.getItem(rememberedKey) ?? '[]') as RememberedUser[]; } catch { return []; }
}

export function rememberUser(user: AppUser) {
  const next = [user, ...loadRememberedUsers().filter((item) => item.login.toLowerCase() !== user.login.toLowerCase())].slice(0, 8);
  localStorage.setItem(rememberedKey, JSON.stringify(next.map(({ login, displayName }) => ({ login, displayName }))));
}
