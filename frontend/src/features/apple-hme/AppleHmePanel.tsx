import { useCallback, useEffect, useRef, useState, type FormEvent } from 'preact/compat';
import type { Notice } from '../../app-model';
import { AppButton } from '../../components/AppButton';
import { AppInput, AppSelect, AppTextarea } from '../../components/form-controls';
import { CheckCircle, Copy, Globe, Key, LockKey, Plus, WarningCircle } from '../../components/icons';
import { SettingsLinkRow } from '../../components/settings-navigation';
import { api } from '../../services';
import type { Account } from '../../types';

type HmeStatus = {
  accountId: string;
  authorized: boolean;
  connected: boolean;
  icloudWebAuthorized: boolean;
  icloudWebConnected: boolean;
  icloudWebStatusMessage?: string;
  icloudWebLastSuccessfulKeepaliveAt?: string;
  appleAccountAuthorized: boolean;
  appleAccountConnected: boolean;
  appleAccountStatusMessage?: string;
  appleAccountLastSuccessfulKeepaliveAt?: string;
  isIcloudPlus: boolean;
  canCreateHme: boolean;
  updatedAt?: string;
};

type HmeAddress = {
  anonymousId: string;
  email: string;
  label: string;
  note: string;
  forwardToEmail: string;
  active: boolean;
  origin: string;
  createdAt?: string;
};

type LoginResult = {
  status: HmeStatus;
  needsTwoFactor: boolean;
  pendingId?: string;
  expiresAt?: string;
  message: string;
};

export type AppleHmeView = 'overview' | 'addresses' | 'create' | 'appleAccountLogin' | 'icloudWebLogin';

function keepaliveLabel(value?: string) {
  return value ? `最近成功保活：${new Date(value).toLocaleString()}` : '尚无成功保活记录';
}

export function AppleHmePanel({ account, setNotice, view, onViewChange }: { account: Account; setNotice: (notice: Notice) => void; view: AppleHmeView; onViewChange: (view: AppleHmeView) => void }) {
  const [status, setStatus] = useState<HmeStatus | null>(null);
  const [addresses, setAddresses] = useState<HmeAddress[]>([]);
  const [lastSyncedAt, setLastSyncedAt] = useState<string | null>(null);
  const [pendingId, setPendingId] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [copiedAddressId, setCopiedAddressId] = useState<string | null>(null);
  const loginFormRef = useRef<HTMLFormElement>(null);
  const copyResetTimerRef = useRef<number | null>(null);
  const loginKind = view === 'icloudWebLogin' ? 'icloudWeb' : view === 'appleAccountLogin' ? 'appleAccount' : null;

  const loadAddresses = useCallback(async () => {
    const result = await api<{ addresses: HmeAddress[]; lastSyncedAt?: string | null }>(`/api/accounts/${account.id}/apple-hme/addresses`);
    setAddresses(result.addresses);
    setLastSyncedAt(result.lastSyncedAt ?? null);
  }, [account.id]);

  const loadStatus = useCallback(async () => {
    try {
      const next = await api<HmeStatus>(`/api/accounts/${account.id}/apple-hme`);
      setStatus(next);
      await loadAddresses();
    } catch (value) {
      setError(value instanceof Error ? value.message : '无法读取 Hide My Email 状态');
    }
  }, [account.id, loadAddresses]);

  useEffect(() => {
    void loadStatus();
    const timer = window.setInterval(() => { void loadStatus(); }, 15_000);
    return () => window.clearInterval(timer);
  }, [loadStatus]);

  useEffect(() => () => {
    if (copyResetTimerRef.current !== null) window.clearTimeout(copyResetTimerRef.current);
  }, []);

  useEffect(() => {
    if (!loginKind) { setPendingId(''); return; }
    const frame = window.requestAnimationFrame(() => {
      loginFormRef.current?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [loginKind, pendingId]);

  async function syncAddresses() {
    setBusy(true); setError('');
    try {
      const result = await api<{ addresses: HmeAddress[]; lastSyncedAt: string }>(`/api/accounts/${account.id}/apple-hme/addresses/sync`, { method: 'POST' });
      setAddresses(result.addresses);
      setLastSyncedAt(result.lastSyncedAt);
      setNotice({ kind: 'success', text: `已从 Apple 同步 ${result.addresses.length} 个隐藏邮件地址` });
    } catch (value) {
      setError(value instanceof Error ? value.message : '隐藏邮件地址同步失败');
      await loadStatus();
    } finally { setBusy(false); }
  }

  async function startLogin(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!loginKind) return;
    const form = new FormData(event.currentTarget);
    setBusy(true); setError('');
    try {
      const result = await api<LoginResult>(`/api/accounts/${account.id}/apple-hme/login`, {
        method: 'POST',
        body: JSON.stringify({
          kind: loginKind,
          appleId: form.get('appleId'),
          password: form.get('password'),
          twoFactorMethod: 'trustedDevice',
        }),
      });
      setStatus(result.status);
      if (result.needsTwoFactor && result.pendingId) {
        setPendingId(result.pendingId);
        setNotice({ kind: 'success', text: 'Apple 已发送双重认证验证码' });
      } else {
        onViewChange('overview');
        setNotice({ kind: 'success', text: 'Apple HME 授权已保存' });
        if (result.status.icloudWebConnected) await loadAddresses();
      }
    } catch (value) {
      setError(value instanceof Error ? value.message : 'Apple 授权失败');
    } finally { setBusy(false); }
  }

  async function submitCode(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true); setError('');
    try {
      const result = await api<LoginResult>(`/api/accounts/${account.id}/apple-hme/two-factor`, {
        method: 'POST', body: JSON.stringify({ pendingId, code: form.get('code') }),
      });
      setStatus(result.status); setPendingId(''); onViewChange('overview');
      setNotice({ kind: 'success', text: 'Apple 双重认证已完成' });
      if (result.status.icloudWebConnected) await loadAddresses();
    } catch (value) {
      setError(value instanceof Error ? value.message : '验证码验证失败');
    } finally { setBusy(false); }
  }

  async function createAddress(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    setBusy(true); setError('');
    try {
      const result = await api<{ address: HmeAddress }>(`/api/accounts/${account.id}/apple-hme/addresses`, {
        method: 'POST', body: JSON.stringify({ label: form.get('label'), note: form.get('note'), channel: form.get('channel') }),
      });
      setAddresses((current) => [result.address, ...current.filter((item) => item.anonymousId !== result.address.anonymousId)]);
      formElement.reset();
      onViewChange('addresses');
      setNotice({ kind: 'success', text: `已创建 ${result.address.email}` });
    } catch (value) {
      setError(value instanceof Error ? value.message : 'Hide My Email 创建失败');
      await loadStatus();
    } finally { setBusy(false); }
  }

  async function mutateAddress(address: HmeAddress, action: 'deactivate' | 'delete') {
    setBusy(true); setError('');
    try {
      const suffix = action === 'deactivate' ? '/deactivate' : '';
      await api(`/api/accounts/${account.id}/apple-hme/addresses/${encodeURIComponent(address.anonymousId)}${suffix}`, { method: action === 'deactivate' ? 'POST' : 'DELETE' });
      await loadAddresses();
      setNotice({ kind: 'success', text: action === 'deactivate' ? `${address.email} 已停用` : `${address.email} 已永久删除` });
    } catch (value) {
      setError(value instanceof Error ? value.message : 'Hide My Email 操作失败');
      await loadStatus();
    } finally { setBusy(false); }
  }

  async function copyAddress(address: HmeAddress) {
    try {
      await navigator.clipboard.writeText(address.email);
      if (copyResetTimerRef.current !== null) window.clearTimeout(copyResetTimerRef.current);
      setCopiedAddressId(address.anonymousId);
      copyResetTimerRef.current = window.setTimeout(() => {
        setCopiedAddressId(null);
        copyResetTimerRef.current = null;
      }, 2_000);
    } catch {
      setNotice({ kind: 'error', text: '复制失败，请手动选择隐私邮箱地址' });
    }
  }

  async function disconnect() {
    setBusy(true); setError('');
    try {
      await api(`/api/accounts/${account.id}/apple-hme`, { method: 'DELETE' });
      setStatus((current) => current ? { ...current, authorized: false, connected: false, icloudWebAuthorized: false, icloudWebConnected: false, icloudWebStatusMessage: undefined, icloudWebLastSuccessfulKeepaliveAt: undefined, appleAccountAuthorized: false, appleAccountConnected: false, appleAccountStatusMessage: undefined, appleAccountLastSuccessfulKeepaliveAt: undefined, isIcloudPlus: false, canCreateHme: false } : current);
      setPendingId(''); onViewChange('overview');
      setNotice({ kind: 'success', text: '本地 Apple HME 会话已删除' });
    } catch (value) {
      setError(value instanceof Error ? value.message : '断开 Apple HME 授权失败');
    } finally { setBusy(false); }
  }

  const bothConnected = Boolean(status?.appleAccountConnected && status?.icloudWebConnected);
  const partiallyConnected = Boolean(status?.appleAccountConnected || status?.icloudWebConnected);
  const overallLabel = !status ? '检查中' : bothConnected ? '全部可用' : partiallyConnected ? '部分可用' : status.authorized ? '需要重新授权' : '未连接';
  const errorNotice = error && <p className="apple-hme-error" role="alert"><WarningCircle size={16} /><span>{error}</span><button type="button" aria-label="关闭错误提示" onClick={() => setError('')}>×</button></p>;

  if (loginKind) return <section className="apple-hme-panel apple-hme-detail-view">
    {errorNotice}
    {!pendingId ? <form key={`apple-hme-credentials-${loginKind}`} ref={loginFormRef} className="apple-hme-login" autoComplete="off" onSubmit={startLogin}>
      <div className="apple-hme-form-heading"><strong>{loginKind === 'icloudWeb' ? '连接 iCloud 地址管理' : '连接 Apple 地址创建'}</strong><small>登录过程中可能需要输入 Apple 设备收到的 6 位验证码。</small></div>
      <AppInput name="appleId" type="email" defaultValue={account.email} aria-label="Apple ID" autoComplete="off" required />
      <AppInput name="password" type="password" placeholder="Apple 账户密码（不会保存）" aria-label="Apple 账户密码" autoComplete="new-password" required />
      <div className="apple-hme-login-actions"><button type="button" onClick={() => onViewChange('overview')}>取消</button><AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '正在连接…' : `授权 ${loginKind === 'icloudWeb' ? 'iCloud Web' : 'Apple Account'}`}</AppButton></div>
    </form> : <form key={`apple-hme-two-factor-${pendingId}`} ref={loginFormRef} className="apple-hme-login" autoComplete="off" onSubmit={submitCode}>
      <div className="apple-hme-form-heading"><strong>完成双重认证</strong><small>输入 Apple 发送到受信任设备的验证码。</small></div>
      <AppInput key={`apple-hme-two-factor-code-${pendingId}`} name="code" type="text" inputMode="numeric" pattern="[0-9]{6}" maxLength={6} placeholder="6 位验证码" aria-label="Apple 双重认证验证码" autoComplete="off" spellcheck={false} autoFocus required />
      <div className="apple-hme-login-actions"><button type="button" onClick={() => { setPendingId(''); onViewChange('overview'); }}>取消</button><AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '正在验证…' : '完成验证'}</AppButton></div>
    </form>}
  </section>;

  if (view === 'addresses') return <section className="apple-hme-panel apple-hme-detail-view">
    {errorNotice}
    <div className="apple-hme-addresses">
      <div className="apple-hme-address-heading"><div><strong>隐私邮箱列表</strong><small>{addresses.length} 个本地地址{lastSyncedAt ? ` · 最后同步 ${new Date(lastSyncedAt).toLocaleString()}` : ' · 尚未从 Apple 同步'}</small></div><div className="apple-hme-address-heading-actions"><AppButton appearance="primary" icon={<Plus size={15} />} disabled={busy} onClick={() => onViewChange('create')}>创建地址</AppButton><AppButton appearance="secondary" disabled={busy} onClick={() => { if (status?.icloudWebConnected) void syncAddresses(); else onViewChange('icloudWebLogin'); }}>{busy ? '同步中…' : status?.icloudWebConnected ? '从 Apple 同步' : '连接 iCloud 后同步'}</AppButton></div></div>
      {addresses.length === 0 ? <div className="apple-hme-empty"><Globe size={21} /><span><strong>本地还没有隐私邮箱</strong><small>{status?.icloudWebConnected ? '点击“从 Apple 同步”获取已创建的隐私邮箱并保存到本地。' : '先连接 iCloud 地址管理，再手动同步 Apple 已创建的隐私邮箱。'}</small></span></div> : addresses.map((address) => <article key={address.anonymousId}>
        <div className="apple-hme-address-details"><span className="apple-hme-address-line"><strong>{address.email}</strong><button type="button" className="apple-hme-copy-address" aria-label={`复制隐私邮箱 ${address.email}`} title="复制隐私邮箱" onClick={() => void copyAddress(address)}><Copy size={15} /></button>{copiedAddressId === address.anonymousId && <span className="apple-hme-copy-success" role="status" aria-label="复制成功" title="复制成功"><CheckCircle size={17} weight="fill" /></span>}</span><small>{address.label || '未命名'}{address.forwardToEmail ? ` · 转发至 ${address.forwardToEmail}` : ''}</small></div>
        <em className={address.active ? 'is-active' : ''}>{address.active ? '使用中' : '已停用'}</em>
        {address.active ? <button type="button" className="apple-hme-address-action" disabled={busy || !status?.icloudWebConnected} onClick={() => void mutateAddress(address, 'deactivate')}>停用</button> : <button type="button" className="apple-hme-address-action apple-hme-delete" disabled={busy || !status?.icloudWebConnected} onClick={() => void mutateAddress(address, 'delete')}>永久删除</button>}
      </article>)}
    </div>
  </section>;

  if (view === 'create') return <section className="apple-hme-panel apple-hme-detail-view">
    {errorNotice}
    {(status?.appleAccountConnected || (status?.icloudWebConnected && status.canCreateHme)) ? <form className="apple-hme-create" onSubmit={createAddress}>
      <div className="apple-hme-form-heading"><strong>创建新的隐私邮箱</strong><small>填写用途和备注后，由 Apple 生成新的转发地址。</small></div>
      <AppInput name="label" placeholder="用途标签，例如：购物账户" aria-label="Hide My Email 标签" maxLength={200} />
      <AppTextarea name="note" placeholder="备注（可选）" aria-label="Hide My Email 备注" maxLength={500} rows={2} />
      <AppSelect name="channel" aria-label="创建通道" defaultValue="auto" options={[{ value: 'auto', label: '自动选择授权通道' }, { value: 'appleAccount', label: 'Apple Account' }, { value: 'icloudWeb', label: 'iCloud Web' }]} />
      <AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '正在创建…' : '创建隐藏邮箱'}</AppButton>
    </form> : <div className="settings-empty"><Key size={34} weight="duotone" /><h3>需要连接 Apple 地址创建</h3><p>完成 Apple Account 授权后才能生成新的隐私邮箱。</p><AppButton appearance="primary" onClick={() => onViewChange('appleAccountLogin')}>连接 Apple Account</AppButton></div>}
  </section>;

  return <section className="apple-hme-panel apple-hme-managed-region" aria-label={`${account.displayName} 隐私邮箱设置`}>
    <header className="apple-hme-heading">
      <span className="apple-hme-mark"><LockKey size={19} /></span>
      <div><strong>{account.displayName}</strong><p>{account.email}</p></div>
      <span className={`apple-hme-status${bothConnected ? ' is-connected' : partiallyConnected ? ' is-partial' : status?.authorized ? ' is-invalid' : ''}`}>
        {bothConnected && <CheckCircle size={14} weight="fill" />}{overallLabel}
      </span>
    </header>
    {errorNotice}
    <div className="apple-hme-account-settings-list">
      <div className="apple-hme-session-row">
        <span><Key size={18} /></span><div><strong>Apple 地址创建</strong><small className="apple-hme-keepalive">{keepaliveLabel(status?.appleAccountLastSuccessfulKeepaliveAt)}</small></div>
        <AppButton className={status?.appleAccountConnected ? 'is-session-connected' : status?.appleAccountAuthorized ? 'is-session-invalid' : ''} appearance={status?.appleAccountConnected ? 'subtle' : 'secondary'} disabled={busy} title={status?.appleAccountConnected ? '点击重新授权 Apple Account' : undefined} onClick={() => { setError(''); setPendingId(''); onViewChange('appleAccountLogin'); }}>{status?.appleAccountConnected ? '已连接' : status?.appleAccountAuthorized ? '重新连接' : '连接'}</AppButton>
      </div>
      <div className="apple-hme-session-row">
        <span><Globe size={18} /></span><div><strong>iCloud 地址管理</strong><small className="apple-hme-keepalive">{keepaliveLabel(status?.icloudWebLastSuccessfulKeepaliveAt)}</small></div>
        <AppButton className={status?.icloudWebConnected ? 'is-session-connected' : status?.icloudWebAuthorized ? 'is-session-invalid' : ''} appearance={status?.icloudWebConnected ? 'subtle' : 'secondary'} disabled={busy} title={status?.icloudWebConnected ? '点击重新授权 iCloud Web' : undefined} onClick={() => { setError(''); setPendingId(''); onViewChange('icloudWebLogin'); }}>{status?.icloudWebConnected ? '已连接' : status?.icloudWebAuthorized ? '重新连接' : '连接'}</AppButton>
      </div>
      <SettingsLinkRow icon={<Globe size={20} />} title="地址管理" detail={lastSyncedAt ? `最后同步 ${new Date(lastSyncedAt).toLocaleString()}` : '查看本地地址，或手动从 Apple 同步。'} value={`${addresses.length} 个`} onClick={() => onViewChange('addresses')} />
      <SettingsLinkRow icon={<Key size={20} />} title="创建地址" detail="填写用途和备注，由 Apple 生成新的转发地址。" value={status?.appleAccountConnected || (status?.icloudWebConnected && status.canCreateHme) ? '可用' : '需要授权'} onClick={() => onViewChange(status?.appleAccountConnected || (status?.icloudWebConnected && status.canCreateHme) ? 'create' : 'appleAccountLogin')} />
    </div>
    {status?.authorized && <footer className="apple-hme-footer"><span>授权会话仅加密保存在这台设备上。</span><button type="button" disabled={busy} onClick={() => void disconnect()}>断开所有 Apple 授权</button></footer>}
  </section>;
}
