import { createContext, useContext } from 'preact/compat';

export type AppUser = { id: string; login: string; displayName: string };
export type AuthContextValue = { user: AppUser; logout: () => Promise<void> };
export const AuthContext = createContext<AuthContextValue | null>(null);

export function useAuth() {
  const value = useContext(AuthContext);
  if (!value) throw new Error('useAuth 必须在 AuthGate 内使用');
  return value;
}
