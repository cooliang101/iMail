import { useEffect, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { Cloud, HardDrives, PlugsConnected } from '@phosphor-icons/react';
import { AppInput } from '../../components/form-controls';
import {
  configuredLocalServicePort,
  configuredLocalServiceUrl,
  configuredRemoteServiceUrl,
  configuredServiceMode,
  normalizeLocalServicePort,
  normalizeServiceUrl,
  saveServiceSelection,
} from '../../service-config';
import { isTauriRuntime } from '../../platform/tauri-runtime';
import { desktopEnableLocalService, desktopEnableLocalServiceAtPort, desktopLocalServiceStatus, desktopPauseLocalService } from '../../local-service';
import { LocalPortRecovery, localPortConflictMessage, suggestedLocalServicePort } from './LocalPortRecovery';
import { serviceErrorMessage, testServiceConnection } from './service-connection';
import { switchToLocalService, switchToRemoteService, type ServiceTransitionDependencies } from './service-transition';

function announceServiceChange() {
  window.dispatchEvent(new Event('imail:service-changed'));
}

export function ServiceAddressEditor({ compact = false, onCancel, onSaved }: { compact?: boolean; onCancel?: () => void; onSaved?: () => void }) {
  const [error, setError] = useState('');
  const [busy, setBusy] = useState<'local' | 'remote' | ''>('');
  const [localPort, setLocalPort] = useState(() => String(configuredLocalServicePort()));
  const [portRecoveryOpen, setPortRecoveryOpen] = useState(false);

  useEffect(() => {
    if (compact && isTauriRuntime()) void desktopLocalServiceStatus().then((status) => {
      if (status.error && localPortConflictMessage(status.error)) {
        const currentPort = configuredLocalServicePort();
        setLocalPort(String(suggestedLocalServicePort(currentPort)));
        setPortRecoveryOpen(true);
      }
    }).catch(() => undefined);
  }, [compact]);

  function transitionDependencies(): ServiceTransitionDependencies {
    return {
      desktop: isTauriRuntime(),
      currentMode: configuredServiceMode,
      testConnection: testServiceConnection,
      enableLocal: (port) => port === undefined ? desktopEnableLocalService() : desktopEnableLocalServiceAtPort(port),
      pauseLocal: desktopPauseLocalService,
      saveSelection: saveServiceSelection,
    };
  }

  async function activateLocal(useSelectedPort = false) {
    setBusy('local'); setError('');
    try {
      const port = useSelectedPort ? normalizeLocalServicePort(localPort) : undefined;
      await switchToLocalService(configuredLocalServiceUrl(), transitionDependencies(), port);
      announceServiceChange(); onSaved?.();
    } catch (reason) {
      setError(`无法使用本地服务：${serviceErrorMessage(reason, '未知错误')}`);
      if (localPortConflictMessage(reason)) {
        if (!portRecoveryOpen) setLocalPort(String(suggestedLocalServicePort(configuredLocalServicePort())));
        setPortRecoveryOpen(true);
      }
    } finally { setBusy(''); }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy('remote'); setError('');
    try {
      const normalized = normalizeServiceUrl(String(new FormData(event.currentTarget).get('serviceUrl') ?? ''));
      if (!normalized) throw new Error('请输入 iMail 服务地址');
      await switchToRemoteService(normalized, transitionDependencies());
      announceServiceChange(); onSaved?.();
    } catch (reason) {
      setError(`无法连接：${serviceErrorMessage(reason, 'iMail 服务不可用')}`);
    } finally { setBusy(''); }
  }

  const desktop = isTauriRuntime();
  return <div className={`service-address-editor ${compact ? 'is-compact' : ''}`}>
    {compact && desktop && <section className="service-local-choice">
      <div className="service-choice-summary"><HardDrives size={21} weight="duotone" /><span><strong>本地服务</strong><small>数据保存在此设备，后台守护进程持续收取邮件</small></span></div>
      <Button appearance="secondary" type="button" onClick={() => void activateLocal()} disabled={Boolean(busy)}>{busy === 'local' && !portRecoveryOpen ? '正在启动…' : '使用本地服务'}</Button>
    </section>}
    {compact && desktop && portRecoveryOpen && <LocalPortRecovery port={localPort} busy={busy === 'local'} onPortChange={setLocalPort} onRetry={() => void activateLocal(true)} />}
    {compact && desktop && <div className="service-choice-divider"><span>或</span></div>}
    <form className="service-remote-form" onSubmit={submit}>
      {compact && desktop && <div className="service-choice-summary"><Cloud size={21} weight="duotone" /><span><strong>远程服务</strong><small>连接你部署的服务，在多台设备间共享同一份数据</small></span></div>}
      <label><span>服务地址</span><AppInput name="serviceUrl" type="url" defaultValue={configuredRemoteServiceUrl()} placeholder="https://mail.example.com" autoFocus={!desktop} required /></label>
      <p className="service-transport-note">请填写 HTTPS 地址；仅本机回环开发地址允许使用 HTTP。</p>
      {error && <div className="auth-error" role="alert">{error}</div>}
      <div className="service-address-actions">
        {onCancel && <button type="button" onClick={onCancel}>取消</button>}
        <Button appearance="primary" type="submit" disabled={Boolean(busy)} icon={<PlugsConnected size={16} />}>{busy === 'remote' ? '正在验证…' : '连接远程服务'}</Button>
      </div>
    </form>
  </div>;
}
