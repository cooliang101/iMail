import { useState, type FormEvent } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { Cloud, HardDrives, PlugsConnected } from '../../components/icons';
import { AppInput } from '../../components/form-controls';
import {
  configuredLocalServiceUrl,
  configuredRemoteServiceUrl,
  normalizeServiceUrl,
  saveServiceSelection,
} from '../../services';
import { isTauriRuntime } from '../../platform/tauri-runtime';
import { serviceErrorMessage, testServiceConnection } from './service-connection';
import { switchToLocalService, switchToRemoteService, type ServiceTransitionDependencies } from './service-transition';

function announceServiceChange() {
  window.dispatchEvent(new Event('imail:service-changed'));
}

export function ServiceAddressEditor({ compact = false, onCancel, onSaved }: { compact?: boolean; onCancel?: () => void; onSaved?: () => void }) {
  const [error, setError] = useState('');
  const [busy, setBusy] = useState<'local' | 'remote' | ''>('');

  function transitionDependencies(): ServiceTransitionDependencies {
    return {
      testConnection: (url) => testServiceConnection(url, { embeddedLocal: url === configuredLocalServiceUrl() }),
      saveSelection: saveServiceSelection,
    };
  }

  async function activateLocal() {
    setBusy('local');
    setError('');
    try {
      await switchToLocalService(configuredLocalServiceUrl(), transitionDependencies());
      announceServiceChange();
      onSaved?.();
    } catch (reason) {
      setError(`无法使用本地服务：${serviceErrorMessage(reason, '未知错误')}`);
    } finally {
      setBusy('');
    }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy('remote');
    setError('');
    try {
      const normalized = normalizeServiceUrl(String(new FormData(event.currentTarget).get('serviceUrl') ?? ''));
      if (!normalized) throw new Error('请输入 iMail 服务地址');
      await switchToRemoteService(normalized, transitionDependencies());
      announceServiceChange();
      onSaved?.();
    } catch (reason) {
      setError(`无法连接：${serviceErrorMessage(reason, 'iMail 服务不可用')}`);
    } finally {
      setBusy('');
    }
  }

  const desktop = isTauriRuntime();
  return <div className={`service-address-editor ${compact ? 'is-compact' : ''}`}>
    {compact && desktop && <section className="service-local-choice">
      <div className="service-choice-summary"><HardDrives size={21} weight="duotone" /><span><strong>本地服务</strong><small>Rust 直接嵌入应用，不开放本地 HTTP 端口</small></span></div>
      <AppButton appearance="secondary" type="button" onClick={() => void activateLocal()} disabled={Boolean(busy)}>{busy === 'local' ? '正在启动…' : '使用本地服务'}</AppButton>
    </section>}
    {compact && desktop && <div className="service-choice-divider"><span>或</span></div>}
    <form className="service-remote-form" onSubmit={submit}>
      {compact && desktop && <div className="service-choice-summary"><Cloud size={21} weight="duotone" /><span><strong>远程服务</strong><small>连接你部署的 Rust HTTP 服务，在多台设备间共享同一份数据</small></span></div>}
      <label><span>服务地址</span><AppInput name="serviceUrl" type="url" defaultValue={configuredRemoteServiceUrl()} placeholder="https://mail.example.com" autoFocus={!desktop} required /></label>
      <p className="service-transport-note">请填写 HTTPS 地址；仅本机回环开发地址允许使用 HTTP。</p>
      {error && <div className="auth-error" role="alert">{error}</div>}
      <div className="service-address-actions">
        {onCancel && <button type="button" onClick={onCancel}>取消</button>}
        <AppButton appearance="primary" type="submit" disabled={Boolean(busy)} icon={<PlugsConnected size={16} />}>{busy === 'remote' ? '正在验证…' : '连接远程服务'}</AppButton>
      </div>
    </form>
  </div>;
}
