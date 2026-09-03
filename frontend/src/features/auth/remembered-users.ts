import type { AppUser } from './auth-context';

export type RememberedUser = Pick<AppUser, 'login' | 'displayName'>;
const rememberedKey = 'imail.remembered-app-users';

type RememberedUserStorage = Pick<Storage, 'getItem' | 'setItem'>;

function defaultStorage() {
  return typeof localStorage === 'undefined' ? undefined : localStorage;
}

export function loadRememberedUsers(storage: Pick<Storage, 'getItem'> | undefined = defaultStorage()): RememberedUser[] {
  if (!storage) return [];
  try {
    const stored: unknown = JSON.parse(storage.getItem(rememberedKey) ?? '[]');
    if (!Array.isArray(stored)) return [];
    return stored.filter((item): item is RememberedUser => Boolean(
      item
      && typeof item === 'object'
      && typeof (item as Partial<RememberedUser>).login === 'string'
      && (item as Partial<RememberedUser>).login?.trim()
      && typeof (item as Partial<RememberedUser>).displayName === 'string',
    ));
  } catch {
    return [];
  }
}

export function rememberUser(user: AppUser, storage: RememberedUserStorage | undefined = defaultStorage()) {
  if (!storage) return;
  const next = [user, ...loadRememberedUsers(storage).filter((item) => item.login.toLowerCase() !== user.login.toLowerCase())].slice(0, 8);
  storage.setItem(rememberedKey, JSON.stringify(next.map(({ login, displayName }) => ({ login, displayName }))));
}
