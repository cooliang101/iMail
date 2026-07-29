import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowClockwise, ArrowRight, CaretDown, Check, Envelope, Gear, Key, Trash, WarningCircle, X } from '@phosphor-icons/react';
import { api } from '../../api';
import { credentialGuideFor, oauthCallbackOrigins } from '../../provider-guides';
import type { Account, ProviderId } from '../../types';
import type { Notice } from '../../app-model';
import { AccountProviderMark, Overlay, ProviderIcon, providerLabel, providers } from '../../components/shared';
import { AppInput, AppSelect, type AppSelectOption } from '../../components/form-controls';

const defaultWorkspaceNames = ['工作', '个人', '对外支持', '开发测试', '同学联系'];

export function AddAccountModal({ accounts, onClose, onAdded }: { accounts: Account[]; onClose: () => void; onAdded: (result?: { warning?: string }) => void | Promise<void> }) {
  const [provider, setProvider] = useState<ProviderId>('outlook');
  const [advanced, setAdvanced] = useState(false);
  const [manualMode, setManualMode] = useState(false);
  const [oauthCatalog, setOauthCatalog] = useState<Array<{ id: string; configured: boolean; redirectUri: string; configurationHint: string }>>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const popupRef = useRef<Window | null>(null);
  const oauthStartedAtRef = useRef(0);
  const oauthOriginsRef = useRef(oauthCallbackOrigins(['http://localhost:8787/api/oauth'], window.location.origin));
  const oauthAccountIdsRef = useRef<Set<string>>(new Set());
  const oauthProviderRef = useRef<ProviderId>('outlook');
  const onAddedRef = useRef(onAdded);
  const selectedProvider = providers.find((item) => item.id === provider)!;
  const oauthStatus = selectedProvider.oauthKey ? oauthCatalog.find((item) => item.id === selectedProvider.oauthKey) : undefined;
  const credentialGuide = credentialGuideFor(provider);
  const usesOAuth = Boolean(selectedProvider.oauthKey && !manualMode);
  const workspaceOptions: AppSelectOption[] = Array.from(new Set([...accounts.map((account) => account.group), ...defaultWorkspaceNames])).map((value) => ({ value, label: value }));

  useEffect(() => { onAddedRef.current = onAdded; }, [onAdded]);

  useEffect(() => {
    void api<{ oauth: Array<{ id: string; configured: boolean; redirectUri: string; configurationHint: string }> }>('/api/providers')
      .then((result) => {
        setOauthCatalog(result.oauth);
        oauthOriginsRef.current = oauthCallbackOrigins(result.oauth.map((item) => item.redirectUri), window.location.origin);
      })
      .catch((value) => setError(value instanceof Error ? value.message : '无法读取 OAuth 配置'));
  }, []);

  useEffect(() => {
    if (provider === 'yahoo' && oauthStatus?.configured === false) setManualMode(true);
  }, [provider, oauthStatus?.configured]);

  useEffect(() => {
    const reconcileOAuthAccount = async (failureMessage: string) => {
      try {
        const result = await api<{ accounts: Account[] }>('/api/accounts');
        const connected = result.accounts.find((account) =>
          !oauthAccountIdsRef.current.has(account.id)
          && account.provider === oauthProviderRef.current
          && account.authMethod === 'oauth2',
        );
        if (connected) {
          await onAddedRef.current(connected.status === 'connected'
            ? undefined
            : { warning: `${connected.email} 的授权已保存；${connected.lastError || '邮件连接仍需重试'}` });
        } else {
          setError(failureMessage);
        }
      } catch {
        setError(failureMessage);
      }
    };
    const receive = (event: MessageEvent) => {
      if (event.source !== popupRef.current || !oauthOriginsRef.current.has(event.origin) || event.data?.source !== 'imail-oauth') return;
      setBusy(false);
      popupRef.current = null;
      if (event.data.success) void onAddedRef.current(event.data.warning ? { warning: event.data.warning } : undefined);
      else {
        void reconcileOAuthAccount(event.data.message || 'OAuth 登录未完成');
      }
    };
    const watchPopup = window.setInterval(() => {
      const popup = popupRef.current;
      if (!popup) return;
      if (Date.now() - oauthStartedAtRef.current > 10 * 60_000) {
        popup.close(); popupRef.current = null; setBusy(false); void reconcileOAuthAccount('授权等待已超时，请重新发起登录。'); return;
      }
      try {
        if (popup.closed) { popupRef.current = null; setBusy(false); void reconcileOAuthAccount('授权窗口已关闭，邮箱尚未添加。你可以检查配置后重试。'); }
      } catch { /* 跨域授权页只需继续等待回调 */ }
    }, 400);
    window.addEventListener('message', receive);
    return () => { window.removeEventListener('message', receive); window.clearInterval(watchPopup); popupRef.current?.close(); };
  }, []);

  function cancelOAuth() {
    popupRef.current?.close(); popupRef.current = null; setBusy(false); setError('已停止等待授权，你可以修改配置或重新登录。');
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('');
    const form = new FormData(event.currentTarget);
    if (usesOAuth) {
      const popup = window.open('', 'imail-oauth', 'popup,width=560,height=720,menubar=no,toolbar=no');
      if (!popup) { setError('浏览器阻止了登录窗口，请允许弹出窗口后重试'); setBusy(false); return; }
      popup.document.write('<title>iMail</title><p style="font-family:system-ui;padding:32px">正在打开安全登录…</p>');
      popupRef.current = popup;
      oauthStartedAtRef.current = Date.now();
      oauthProviderRef.current = provider;
      try {
        const snapshot = await api<{ accounts: Account[] }>('/api/accounts');
        oauthAccountIdsRef.current = new Set(snapshot.accounts.map((account) => account.id));
        const result = await api<{ authorizationUrl: string }>('/api/oauth/start', { method: 'POST', body: JSON.stringify({ provider, displayName: form.get('displayName') || undefined, group: form.get('group'), color: '#168f78' }) });
        popup.location.replace(result.authorizationUrl);
      } catch (value) {
        popup.close(); popupRef.current = null; setBusy(false);
        setError(value instanceof Error ? value.message : '无法开始 OAuth 登录');
      }
      return;
    }
    const body: Record<string, unknown> = { provider, email: form.get('email'), displayName: form.get('displayName'), group: form.get('group'), password: form.get('password'), color: '#168f78' };
    if (provider === 'custom') body.settings = { imapHost: form.get('imapHost'), imapPort: Number(form.get('imapPort')), imapSecure: true, smtpHost: form.get('smtpHost'), smtpPort: Number(form.get('smtpPort')), smtpSecure: Number(form.get('smtpPort')) === 465 };
    try { await api('/api/accounts', { method: 'POST', body: JSON.stringify(body) }); await onAdded(); }
    catch (value) { setError(value instanceof Error ? value.message : '连接失败'); }
    finally { setBusy(false); }
  }
  return <Overlay onClose={onClose} wide><form className="account-modal" onSubmit={submit}>
    <div className="modal-header"><div><span>连接新的收件箱</span><h2>添加邮箱</h2><p>优先使用服务商安全登录，iMail 不会接触你的网页登录密码。</p></div><button type="button" aria-label="关闭添加邮箱窗口" onClick={onClose}><X size={21} /></button></div>
    <div className="provider-grid">{providers.map((item) => <button type="button" key={item.id} disabled={busy} className={provider === item.id ? 'selected' : ''} onClick={() => { const status = item.oauthKey ? oauthCatalog.find((entry) => entry.id === item.oauthKey) : undefined; setProvider(item.id); setManualMode(item.id === 'yahoo' && status?.configured === false); setError(''); }}><i className={`provider-mark provider-${item.id}`}><ProviderIcon provider={item.id} /></i><span>{item.name}</span>{item.oauthKey && <small className="oauth-chip">OAuth</small>}{provider === item.id && <Check size={15} weight="bold" />}</button>)}</div>
    {usesOAuth ? <>
      <div className="oauth-panel">
        <div className={`oauth-status ${oauthStatus?.configured ? 'ready' : 'setup'}`}><Key size={21} weight="duotone" /><span><strong>{providerLabel[provider]} 安全登录</strong><small>{oauthStatus?.configured ? 'OAuth 已配置。登录将在服务商官方页面完成，并自动安全刷新授权。' : oauthStatus?.configurationHint || '正在读取 OAuth 配置…'}</small></span></div>
        <div className="form-grid oauth-profile"><label><span>显示名称（可选）</span><AppInput name="displayName" placeholder="默认使用账户名称" /></label><label><span>加入工作空间</span><AppSelect name="group" defaultValue="工作" options={workspaceOptions} /></label></div>
        {provider === 'yahoo' && <div className="oauth-review"><WarningCircle size={17} /><span>Yahoo 的 mail-r/mail-w 权限只对审核通过的应用开放。</span></div>}
        {busy && <div className="oauth-waiting"><span><strong>正在等待 {providerLabel[provider]} 授权</strong><small>如果服务商页面显示配置错误，请关闭授权窗口或结束等待，修正后可以直接重试。</small></span><button type="button" onClick={cancelOAuth}>结束等待</button></div>}
      </div>
      {credentialGuide && <button type="button" className="manual-switch" onClick={() => setManualMode(true)}>无法使用 OAuth？改用官方应用专用密码</button>}
    </> : <>
      {credentialGuide && <section className="credential-guide">
        <div className="credential-guide-heading"><Key size={21} weight="duotone" /><span><strong>{credentialGuide.title}</strong><small>{credentialGuide.description}</small></span><a href={credentialGuide.helpUrl} target="_blank" rel="noreferrer">{credentialGuide.actionLabel}<ArrowRight size={14} /></a></div>
        <ol>{credentialGuide.steps.map((step, index) => <li key={step}><b>{index + 1}</b><span>{step}</span></li>)}</ol>
      </section>}
      <div className="form-grid"><label><span>邮箱地址</span><AppInput name="email" type="email" placeholder="name@example.com" required /></label><label><span>显示名称</span><AppInput name="displayName" placeholder="例如：工作邮箱" required /></label><label><span>工作空间</span><AppSelect name="group" defaultValue="工作" options={workspaceOptions} /></label><label><span>{credentialGuide?.secretLabel || '应用专用密码 / 授权码'}</span><AppInput name="password" type="password" placeholder={credentialGuide?.secretPlaceholder || '不会以明文保存'} required /></label></div>
      {provider !== 'custom' && !credentialGuide && <div className="provider-tip"><Key size={19} /><span><strong>{providerLabel[provider]} 安全提示</strong><small>请使用服务商提供的专用凭据，不要填写网页登录密码。</small></span></div>}
      {selectedProvider.oauthKey && oauthStatus?.configured && <button type="button" className="manual-switch" onClick={() => setManualMode(false)}>返回 {providerLabel[provider]} OAuth 安全登录</button>}
    </>}
    {provider === 'custom' && <div className="advanced-settings"><button type="button" onClick={() => setAdvanced(!advanced)}><Gear size={17} />IMAP / SMTP 设置<CaretDown size={15} /></button>{(advanced || provider === 'custom') && <div className="form-grid"><label><span>IMAP 主机</span><AppInput name="imapHost" placeholder="imap.example.com" required /></label><label><span>IMAP 端口</span><AppInput name="imapPort" type="number" defaultValue="993" required /></label><label><span>SMTP 主机</span><AppInput name="smtpHost" placeholder="smtp.example.com" required /></label><label><span>SMTP 端口</span><AppInput name="smtpPort" type="number" defaultValue="465" required /></label></div>}</div>}
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
    <div className="modal-footer"><button type="button" onClick={onClose}>取消</button><Button appearance="primary" type="submit" disabled={busy || (usesOAuth && !oauthStatus?.configured)}>{busy ? (usesOAuth ? '等待授权…' : '正在验证连接…') : error && usesOAuth ? `重新使用 ${providerLabel[provider]} 登录` : usesOAuth ? `使用 ${providerLabel[provider]} 登录` : '验证并添加'}</Button></div>
  </form></Overlay>;
}
