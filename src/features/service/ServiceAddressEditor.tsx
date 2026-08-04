import { useEffect, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { CheckCircle, HardDrives, PlugsConnected } from '@phosphor-icons/react';
import { AppInput } from '../../components/form-controls';
import {
  configuredRemoteServiceUrl,
  configuredServiceMode,
  LOCAL_SERVICE_URL,
  normalizeServiceUrl,
  saveServiceSelection,
} from '../../service-config';
import { isTauriRuntime } from '../../platform/tauri-runtime';
import { desktopEnableLocalService, desktopLocalServiceStatus, desktopPauseLocalService, type LocalServiceStatus } from '../../local-service';
import { LocalDataDeletion } from './LocalDataDeletion';
import { serviceErrorMessage, testServiceConnection } from './service-connection';
import { switchToLocalService, switchToRemoteService, type ServiceTransitionDependencies } from './service-transition';

function announceServiceChange() {
  window.dispatchEvent(new Event('imail:service-changed'));
}

export function ServiceAddressEditor({ compact = false, onCancel, onSaved }: { compact?: boolean; onCancel?: () => void; onSaved?: () => void }) {
  const [error, setError] = useState('');
  const [busy, setBusy] = useState<'local' | 'remote' | ''>('');
  const [localStatus, setLocalStatus] = useState<LocalServiceStatus>();

  useEffect(() => {
    if (compact && isTauriRuntime()) void desktopLocalServiceStatus().then(setLocalStatus).catch(() => undefined);
  }, [compact]);

  function transitionDependencies(): ServiceTransitionDependencies {
    return {
      desktop: isTauriRuntime(),
      currentMode: configuredServiceMode,
      testConnection: testServiceConnection,
      enableLocal: desktopEnableLocalService,
      pauseLocal: desktopPauseLocalService,
      saveSelection: saveServiceSelection,
    };
  }

  async function activateLocal() {
    setBusy('local'); setError('');
    try {
      await switchToLocalService(LOCAL_SERVICE_URL, transitionDependencies());
      announceServiceChange(); onSaved?.();
    } catch (reason) {
      setError(`无法使用本地服务：${serviceErrorMessage(reason, '未知错误')}`);
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

  return <form className={`service-address-editor ${compact ? 'is-compact' : ''}`} onSubmit={submit}>
    <label><span><CheckCircle size={15} weight="fill" />远程服务地址</span><AppInput name="serviceUrl" type="url" defaultValue={configuredRemoteServiceUrl()} placeholder="https://mail.example.com" autoFocus required /></label>
    <p className="service-transport-note">远程服务必须使用 HTTPS；HTTP 仅允许本机回环开发地址。</p>
    {error && <div className="auth-error" role="alert">{error}</div>}
    <div className="service-address-actions">
      {compact && isTauriRuntime() && <button type="button" onClick={() => void activateLocal()} disabled={Boolean(busy)}><HardDrives size={15} />{busy === 'local' ? '正在检查…' : '使用本地服务'}</button>}
      {onCancel && <button type="button" onClick={onCancel}>取消</button>}
      <Button appearance="primary" type="submit" disabled={Boolean(busy)} icon={<PlugsConnected size={16} />}>{busy === 'remote' ? '正在验证…' : '连接远程服务'}</Button>
    </div>
    {compact && localStatus?.dataPresent && <LocalDataDeletion status={localStatus} onDeleted={(next) => { setLocalStatus(next); announceServiceChange(); }} />}
  </form>;
}
