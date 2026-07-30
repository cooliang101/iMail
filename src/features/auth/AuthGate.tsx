import { createContext, useContext, useEffect, useMemo, useState, type FormEvent, type ReactNode } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowLeft, LockKey, UserCircle, UserPlus } from '@phosphor-icons/react';
import { api } from '../../api';
import { AppInput } from '../../components/form-controls';

type User = { id: string; login: string; displayName: string };
type RememberedUser = Pick<User, 'login' | 'displayName'>;
type AuthContextValue = { user: User; logout: () => Promise<void> };
const AuthContext = createContext<AuthContextValue | null>(null);
const rememberedKey = 'imail.remembered-app-users';

function loadRemembered(): RememberedUser[] {
  try { return JSON.parse(localStorage.getItem(rememberedKey) ?? '[]') as RememberedUser[]; } catch { return []; }
}
function remember(user: User) {
  const next = [user, ...loadRemembered().filter((item) => item.login.toLowerCase() !== user.login.toLowerCase())].slice(0, 8);
  localStorage.setItem(rememberedKey, JSON.stringify(next.map(({ login, displayName }) => ({ login, displayName }))));
}

export function useAuth() {
  const value = useContext(AuthContext);
  if (!value) throw new Error('useAuth 必须在 AuthGate 内使用');
  return value;
}

export function AuthGate({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [checking, setChecking] = useState(true);
  const [setupRequired, setSetupRequired] = useState(false);
  const [mode, setMode] = useState<'login' | 'register'>('login');
  const [selectedLogin, setSelectedLogin] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [remembered, setRemembered] = useState(loadRemembered);

  async function checkSession() {
    try {
      const status = await api<{ setupRequired: boolean; user: User | null }>('/api/auth/status');
      setSetupRequired(status.setupRequired); setMode(status.setupRequired ? 'register' : 'login'); setUser(status.user);
      if (status.user) remember(status.user);
    } catch { setUser(null); setError('无法连接 iMail 服务'); }
    finally { setChecking(false); }
  }
  useEffect(() => { void checkSession(); }, []);
  useEffect(() => {
    const unauthorized = () => { setUser(null); setMode('login'); setError('登录已过期，请重新登录'); };
    window.addEventListener('imail:unauthorized', unauthorized);
    return () => window.removeEventListener('imail:unauthorized', unauthorized);
  }, []);

  async function logout() {
    await api('/api/auth/logout', { method: 'POST' }).catch(() => undefined);
    setUser(null); setMode('login'); setSelectedLogin(user?.login ?? ''); setError('');
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('');
    const form = new FormData(event.currentTarget);
    const body = Object.fromEntries(form.entries());
    try {
      const result = await api<{ user: User }>(mode === 'register' ? '/api/auth/register' : '/api/auth/login', { method: 'POST', body: JSON.stringify(body) });
      remember(result.user); setRemembered(loadRemembered()); setUser(result.user); setSetupRequired(false);
    } catch (reason) { setError(reason instanceof Error ? reason.message : '操作失败'); }
    finally { setBusy(false); }
  }

  const context = useMemo(() => user ? { user, logout } : null, [user]);
  if (checking) return <main className="auth-loading"><img src="/brand/imail-app-icon.png" alt="iMail" /><span>正在检查登录状态…</span></main>;
  if (user && context) return <AuthContext.Provider value={context}>{children}</AuthContext.Provider>;

  const switcherVisible = mode === 'login' && remembered.length > 0 && !selectedLogin;
  return <main className="auth-page">
    <section className="auth-brand-panel">
      <img src="/brand/imail-app-icon.png" alt="iMail" />
      <span>LOCAL-FIRST MAIL</span>
      <h1>你的邮件，<br />只属于你的账号。</h1>
      <p>应用账号将邮箱、邮件、草稿和访问令牌隔离开来。邮件凭据仍只在本机加密保存。</p>
    </section>
    <section className="auth-card-wrap">
      <div className="auth-card">
        <header>
          <small>{mode === 'register' ? '创建应用账号' : '安全登录'}</small>
          <h2>{setupRequired ? '先创建你的账号' : mode === 'register' ? '创建另一个账号' : '欢迎回来'}</h2>
          <p>{setupRequired ? '这是首次使用 iMail。创建后，现有本地邮件将安全归属于你。' : mode === 'register' ? '新账号拥有独立的邮箱与邮件空间。' : '选择一个账号，或使用登录名继续。'}</p>
        </header>

        {switcherVisible && <div className="account-switcher" aria-label="选择账号">
          {remembered.map((item) => <button key={item.login} type="button" onClick={() => setSelectedLogin(item.login)}>
            <UserCircle size={30} weight="duotone" /><span><strong>{item.displayName}</strong><small>{item.login}</small></span><span>继续</span>
          </button>)}
          <button className="use-another-account" type="button" onClick={() => setSelectedLogin('__manual__')}><UserPlus size={22} />使用其他账号</button>
        </div>}

        {!switcherVisible && <form onSubmit={submit}>
          {mode === 'login' && selectedLogin && selectedLogin !== '__manual__' && <button className="auth-back" type="button" onClick={() => setSelectedLogin('')}><ArrowLeft size={15} />切换账号</button>}
          {mode === 'register' && <label><span>显示名称</span><AppInput name="displayName" autoComplete="name" placeholder="例如：林墨" required /></label>}
          <label><span>登录名</span><AppInput name="login" autoComplete="username" defaultValue={selectedLogin === '__manual__' ? '' : selectedLogin} placeholder="用户名或邮箱" required /></label>
          <label><span>密码</span><AppInput name="password" type="password" autoComplete={mode === 'register' ? 'new-password' : 'current-password'} placeholder="至少 8 个字符" required /></label>
          {error && <div className="auth-error" role="alert">{error}</div>}
          <Button appearance="primary" type="submit" disabled={busy} icon={<LockKey size={18} />}>{busy ? '请稍候…' : mode === 'register' ? '创建并进入 iMail' : '登录 iMail'}</Button>
        </form>}

        {!setupRequired && <footer>{mode === 'login' ? '需要独立空间？' : '已经有账号？'} <button type="button" onClick={() => { setMode(mode === 'login' ? 'register' : 'login'); setSelectedLogin(''); setError(''); }}>{mode === 'login' ? '创建新账号' : '返回登录'}</button></footer>}
      </div>
    </section>
  </main>;
}
