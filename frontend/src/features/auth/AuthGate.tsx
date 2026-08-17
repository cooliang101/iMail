import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { ArrowLeft, HardDrives, LockKey, UserCircle, UserPlus } from '../../components/icons';
import { api, configuredServiceMode, configuredServiceUrl, describeDesktopLogValue, desktopLog } from '../../services';
import { AppInput } from '../../components/form-controls';
import { BrandLogo } from '../../components/brand-logo';
import { AuthContext, type AppUser } from './auth-context';
import { loadRememberedUsers, rememberUser } from './remembered-users';
import { createLatestServiceCheckRunner, runWithReadySelectedService, serviceErrorMessage, ServiceAddressEditor, testServiceConnection } from '../service';
import { isTauriRuntime } from '../../platform/tauri-runtime';

export function AuthGate({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<AppUser | null>(null);
  const [checking, setChecking] = useState(true);
  const [setupRequired, setSetupRequired] = useState(false);
  const [registrationOpen, setRegistrationOpen] = useState(true);
  const [mode, setMode] = useState<'login' | 'register'>('login');
  const [selectedLogin, setSelectedLogin] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [remembered, setRemembered] = useState(loadRememberedUsers);
  const [serviceSettingsOpen, setServiceSettingsOpen] = useState(false);
  const serviceCheckRunner = useRef(createLatestServiceCheckRunner()).current;

  const checkSession = useCallback(async () => {
    await serviceCheckRunner.run(() => runWithReadySelectedService({
        mode: configuredServiceMode(),
        serviceUrl: configuredServiceUrl(),
        testConnection: testServiceConnection,
      }, () => api<{ setupRequired: boolean; registrationOpen: boolean; user: AppUser | null }>('/api/auth/status')), {
      onSuccess(status) {
      setSetupRequired(status.setupRequired); setRegistrationOpen(status.registrationOpen); setMode(status.setupRequired ? 'register' : 'login'); setUser(status.user);
      if (status.user) rememberUser(status.user);
      },
      onError(reason) {
        void desktopLog('error', 'startup.service_failed', describeDesktopLogValue(reason));
        setUser(null); setError(serviceErrorMessage(reason, '无法连接 iMail 服务'));
      },
      onSettled() { setChecking(false); },
    });
  }, [serviceCheckRunner]);
  useEffect(() => { void checkSession(); }, [checkSession]);
  useEffect(() => {
    const serviceChanged = () => { setChecking(true); setUser(null); void checkSession(); };
    window.addEventListener('imail:service-changed', serviceChanged);
    return () => window.removeEventListener('imail:service-changed', serviceChanged);
  }, [checkSession]);
  useEffect(() => {
    const unauthorized = () => { serviceCheckRunner.cancel(); setChecking(false); setUser(null); setMode('login'); setError('登录已过期，请重新登录'); };
    window.addEventListener('imail:unauthorized', unauthorized);
    return () => window.removeEventListener('imail:unauthorized', unauthorized);
  }, [serviceCheckRunner]);
  useEffect(() => {
    if (checking || !isTauriRuntime()) return;
    void import('@tauri-apps/api/core')
      .then(({ invoke }) => invoke('desktop_frontend_ready'))
      .catch((reason) => console.error('[desktop-ready]', reason));
  }, [checking]);

  const logout = useCallback(async () => {
    serviceCheckRunner.cancel();
    await api('/api/auth/logout', { method: 'POST' })
      .catch((reason) => desktopLog('warn', 'auth.logout_request_failed', describeDesktopLogValue(reason)));
    setUser(null); setMode('login'); setSelectedLogin(user?.login ?? ''); setError('');
  }, [serviceCheckRunner, user?.login]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('');
    const form = new FormData(event.currentTarget);
    const body = Object.fromEntries(form.entries());
    try {
      const result = await api<{ user: AppUser }>(mode === 'register' ? '/api/auth/register' : '/api/auth/login', { method: 'POST', body: JSON.stringify(body) });
      rememberUser(result.user); setRemembered(loadRememberedUsers()); setUser(result.user); setSetupRequired(false);
    } catch (reason) { setError(reason instanceof Error ? reason.message : '操作失败'); }
    finally { setBusy(false); }
  }

  const context = useMemo(() => user ? { user, logout } : null, [logout, user]);
  if (checking) return <main className="auth-loading"><BrandLogo label="iMail" /><span>正在检查登录状态…</span></main>;
  if (user && context) return <AuthContext.Provider value={context}>{children}</AuthContext.Provider>;

  const switcherVisible = mode === 'login' && remembered.length > 0 && !selectedLogin;
  return <main className="auth-page">
    <section className="auth-brand-panel">
      <BrandLogo label="iMail" />
      <span>ONE APP · EVERY INBOX</span>
      <h1>一个应用，<br />所有邮箱，<br />通用规则。</h1>
      <p>把多个邮箱放进一个工作区，统一查看、统一处理、统一设置。邮件凭据由你选择的 iMail 服务加密保存。</p>
    </section>
    <section className="auth-card-wrap">
      <div className={`auth-card ${serviceSettingsOpen ? 'is-service-view' : ''}`}>
        <header>
          <small>{serviceSettingsOpen ? '服务连接' : mode === 'register' ? '创建应用账号' : '安全登录'}</small>
          <h2>{serviceSettingsOpen ? '选择数据服务' : setupRequired ? '先创建你的账号' : mode === 'register' ? '创建另一个账号' : '欢迎回来'}</h2>
          <p>{serviceSettingsOpen ? '使用此设备上的后台服务，或连接用于多设备共享的远程服务。' : setupRequired ? '这是首次使用 iMail。创建后，现有本地邮件将安全归属于你。' : mode === 'register' ? '新账号拥有独立的邮箱与邮件空间。' : '选择一个账号，或使用登录名继续。'}</p>
        </header>
        {!serviceSettingsOpen && switcherVisible && <div className="account-switcher" aria-label="选择账号">
          {remembered.map((item) => <button key={item.login} type="button" onClick={() => setSelectedLogin(item.login)}>
            <UserCircle size={30} weight="duotone" /><span><strong>{item.displayName}</strong><small>{item.login}</small></span><span>继续</span>
          </button>)}
          <button className="use-another-account" type="button" onClick={() => setSelectedLogin('__manual__')}><UserPlus size={22} />使用其他账号</button>
        </div>}

        {!serviceSettingsOpen && !switcherVisible && <form onSubmit={submit}>
          {mode === 'login' && selectedLogin && selectedLogin !== '__manual__' && <button className="auth-back" type="button" onClick={() => setSelectedLogin('')}><ArrowLeft size={15} />切换账号</button>}
          {mode === 'register' && <label><span>显示名称</span><AppInput name="displayName" autoComplete="name" placeholder="例如：林墨" required /></label>}
          <label><span>登录名</span><AppInput name="login" autoComplete="username" defaultValue={selectedLogin === '__manual__' ? '' : selectedLogin} placeholder="用户名或邮箱" required /></label>
          <label><span>密码</span><AppInput name="password" type="password" autoComplete={mode === 'register' ? 'new-password' : 'current-password'} placeholder="至少 8 个字符" required /></label>
          {error && <div className="auth-error" role="alert">{error}</div>}
          <AppButton appearance="primary" type="submit" disabled={busy} icon={<LockKey size={18} />}>{busy ? '请稍候…' : mode === 'register' ? '创建并进入 iMail' : '登录 iMail'}</AppButton>
        </form>}

        {serviceSettingsOpen && <ServiceAddressEditor compact onSaved={() => setServiceSettingsOpen(false)} />}
        <footer className="auth-card-footer">
          {serviceSettingsOpen ? <button className="auth-service-back" type="button" onClick={() => { setServiceSettingsOpen(false); setError(''); }}><ArrowLeft size={14} />返回登录</button> : <>
            {!setupRequired && (registrationOpen || mode === 'register') && <span>{mode === 'login' ? '需要独立空间？' : '已经有账号？'} <button type="button" onClick={() => { setMode(mode === 'login' ? 'register' : 'login'); setSelectedLogin(''); setError(''); }}>{mode === 'login' ? '创建新账号' : '返回登录'}</button></span>}
            <button className="auth-remote-trigger" type="button" aria-expanded="false" onClick={() => { setServiceSettingsOpen(true); setError(''); }}><HardDrives size={14} />服务连接</button>
          </>}
        </footer>
      </div>
    </section>
  </main>;
}
