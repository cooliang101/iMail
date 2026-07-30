import { AsyncLocalStorage } from 'node:async_hooks';

type AuthContext = { userId: string };

const authContext = new AsyncLocalStorage<AuthContext>();

export function currentUserId() { return authContext.getStore()?.userId; }

export function withUserContext<T>(userId: string, callback: () => T): T {
  return authContext.run({ userId }, callback);
}

export function enterUserContext(userId: string) { authContext.enterWith({ userId }); }

export function userMetadataKey(key: string, userId = currentUserId()) {
  return userId ? `user:${userId}:${key}` : key;
}
