import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { WarningCircle, X } from '@phosphor-icons/react';
import { api } from '../../api';
import { credentialGuideFor, oauthCallbackOrigins } from '../../provider-guides';
import type { Account, ProviderId } from '../../types';
import type { Notice } from '../../app-model';
import { Overlay, providerLabel, providers } from '../../components/shared';
import type { AppSelectOption } from '../../components/form-controls';
import { AccountConnectionFields } from './AccountConnectionFields';
import { ProviderPicker } from './ProviderPicker';
import { usePlatform } from '../../platform/runtime';

const defaultWorkspaceNames = ['工作', '个人', '对外支持', '开发测试', '同学联系'];

export function AddAccountModal({ accounts, onClose, onAdded }: { accounts: Account[]; onClose: () => void; onAdded: (result?: { warning?: string }) => void | Promise<void> }) {
  const platform = usePlatform();
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
  const oauthCancelledRef = useRef(false);
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
      .catch(() => setError('暂时无法读取快捷登录配置，请稍后重试。'));
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
        void reconcileOAuthAccount(event.data.message || '邮箱登录未完成');
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
    oauthCancelledRef.current = true;
    popupRef.current?.close(); popupRef.current = null; setBusy(false); setError('已停止等待授权，你可以修改配置或重新登录。');
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('');
    const form = new FormData(event.currentTarget);
    if (usesOAuth) {
      oauthCancelledRef.current = false;
      oauthStartedAtRef.current = Date.now();
      oauthProviderRef.current = provider;
      if (platform.kind === 'tauri') {
        try {
          const snapshot = await api<{ accounts: Account[] }>('/api/accounts');
          oauthAccountIdsRef.current = new Set(snapshot.accounts.map((account) => account.id));
          const result = await api<{ authorizationUrl: string }>('/api/oauth/start', { method: 'POST', body: JSON.stringify({ provider, displayName: form.get('displayName') || undefined, group: form.get('group'), color: '#168f78' }) });
          await platform.openExternal(result.authorizationUrl);
          while (!oauthCancelledRef.current && Date.now() - oauthStartedAtRef.current <= 10 * 60_000) {
            await new Promise((resolve) => window.setTimeout(resolve, 1_000));
            const current = await api<{ accounts: Account[] }>('/api/accounts');
            const connected = current.accounts.find((account) => !oauthAccountIdsRef.current.has(account.id) && account.provider === oauthProviderRef.current && account.authMethod === 'oauth2');
            if (!connected) continue;
            setBusy(false);
            await onAddedRef.current(connected.status === 'connected' ? undefined : { warning: `${connected.email} 的授权已保存；${connected.lastError || '邮件连接仍需重试'}` });
            return;
          }
          if (!oauthCancelledRef.current) setError('授权等待已超时，请重新发起登录。');
        } catch (value) {
          if (!oauthCancelledRef.current) setError(value instanceof Error ? value.message : '无法打开邮箱登录，请稍后重试。');
        } finally {
          setBusy(false);
        }
        return;
      }
      const popup = window.open('', 'imail-oauth', 'popup,width=560,height=720,menubar=no,toolbar=no');
      if (!popup) { setError('浏览器阻止了登录窗口，请允许弹出窗口后重试'); setBusy(false); return; }
      popup.document.write('<title>iMail</title><p style="font-family:system-ui;padding:32px">正在打开安全登录…</p>');
      popupRef.current = popup;
      try {
        const snapshot = await api<{ accounts: Account[] }>('/api/accounts');
        oauthAccountIdsRef.current = new Set(snapshot.accounts.map((account) => account.id));
        const result = await api<{ authorizationUrl: string }>('/api/oauth/start', { method: 'POST', body: JSON.stringify({ provider, displayName: form.get('displayName') || undefined, group: form.get('group'), color: '#168f78' }) });
        popup.location.replace(result.authorizationUrl);
      } catch (value) {
        popup.close(); popupRef.current = null; setBusy(false);
        setError(value instanceof Error ? value.message : '无法打开邮箱登录，请稍后重试。');
      }
      return;
    }
    const body: Record<string, unknown> = { provider, email: form.get('email'), displayName: form.get('displayName'), group: form.get('group'), password: form.get('password'), color: '#168f78' };
    if (provider === 'custom') body.settings = { imapHost: form.get('imapHost'), imapPort: Number(form.get('imapPort')), imapSecure: true, smtpHost: form.get('smtpHost'), smtpPort: Number(form.get('smtpPort')), smtpSecure: Number(form.get('smtpPort')) === 465 };
    try { await api('/api/accounts', { method: 'POST', body: JSON.stringify(body) }); await onAdded(); }
    catch (value) { setError(value instanceof Error ? value.message : '连接失败'); }
    finally { setBusy(false); }
  }
  return <Overlay onClose={onClose} wide dialogClassName="account-modal-shell"><form className="account-modal" onSubmit={submit}>
    <div className="modal-header"><div><span>连接新的收件箱</span><h2>添加邮箱</h2><p>选择你的邮箱平台，登录后即可在 iMail 中统一收发邮件。</p></div><button type="button" aria-label="关闭添加邮箱窗口" onClick={onClose}><X size={21} /></button></div>
    <ProviderPicker value={provider} busy={busy} onChange={(nextProvider) => { const item = providers.find((candidate) => candidate.id === nextProvider)!; const status = item.oauthKey ? oauthCatalog.find((entry) => entry.id === item.oauthKey) : undefined; setProvider(nextProvider); setManualMode(nextProvider === 'yahoo' && status?.configured === false); setError(''); }} />
    <AccountConnectionFields provider={provider} usesOAuth={usesOAuth} oauthConfigured={oauthStatus?.configured} credentialGuide={credentialGuide} workspaceOptions={workspaceOptions} busy={busy} advanced={advanced} onAdvancedChange={setAdvanced} onManualModeChange={setManualMode} onCancelOAuth={cancelOAuth} />
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
    <div className="modal-footer"><button type="button" onClick={onClose}>取消</button><Button appearance="primary" type="submit" disabled={busy || (usesOAuth && !oauthStatus?.configured)}>{busy ? (usesOAuth ? '等待授权…' : '正在验证连接…') : error && usesOAuth ? `重新使用 ${providerLabel[provider]} 登录` : usesOAuth ? `使用 ${providerLabel[provider]} 登录` : '验证并添加'}</Button></div>
  </form></Overlay>;
}
