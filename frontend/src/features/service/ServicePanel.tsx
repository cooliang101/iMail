import { useCallback, useEffect, useState } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { SettingsLinkRow } from '../../components/settings-navigation';
import { CheckCircle, Cloud, FolderOpen, HardDrives, SpinnerGap, WarningCircle } from '../../components/icons';
import type { ServiceInfo } from '../../types';
import {
  configuredLocalServiceUrl,
  configuredServiceMode,
  configuredServiceUrl,
  saveServiceSelection,
  embeddedTauriServiceEnabled,
  desktopOpenAppLogs,
  describeDesktopLogValue,
  desktopLog,
  type ServiceMode,
} from '../../services';
import { isTauriRuntime } from '../../platform/tauri-runtime';
import { serviceErrorMessage, testServiceConnection } from './service-connection';
import { switchToLocalService, type ServiceTransitionDependencies } from './service-transition';

function announceServiceChange() {
  window.dispatchEvent(new Event('imail:service-changed'));
}

function localServiceNote(mode: ServiceMode) {
  return mode === 'local'
    ? 'Rust 服务已直接嵌入桌面进程，不监听本地 HTTP 端口；窗口隐藏后继续同步，显式退出后停止。'
    : '本地 Rust 服务及数据保留在此设备；当前客户端已选择远程服务。';
}

export function ServicePanel({ onEditRemote }: { onEditRemote: () => void }) {
  const desktop = isTauriRuntime();
  const embeddedLocal = desktop && embeddedTauriServiceEnabled();
  const [mode, setMode] = useState<ServiceMode>(configuredServiceMode);
  const [info, setInfo] = useState<ServiceInfo>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  function transitionDependencies(): ServiceTransitionDependencies {
    return {
      testConnection: (url) => testServiceConnection(url, { embeddedLocal: url === configuredLocalServiceUrl() }),
      saveSelection: saveServiceSelection,
    };
  }

  const inspect = useCallback(async (url = configuredServiceUrl()) => {
    setError('');
    try {
      setInfo(await testServiceConnection(url, { embeddedLocal: embeddedLocal && configuredServiceMode() === 'local' }));
    } catch (reason) {
      setInfo(undefined);
      setError(serviceErrorMessage(reason, '服务不可用'));
      void desktopLog('warn', 'service.inspect_failed', describeDesktopLogValue(reason));
    }
  }, [embeddedLocal]);

  useEffect(() => { void inspect(); }, [inspect]);

  async function activateLocal() {
    setBusy(true);
    setError('');
    try {
      const nextInfo = await switchToLocalService(configuredLocalServiceUrl(), transitionDependencies());
      setMode('local');
      setInfo(nextInfo);
      announceServiceChange();
    } catch (reason) {
      void desktopLog('error', 'service.activate_failed', describeDesktopLogValue(reason));
      setError(`本地服务尚未就绪：${serviceErrorMessage(reason, '未知错误')}`);
    } finally {
      setBusy(false);
    }
  }

  const address = configuredServiceUrl();
  return <section className="settings-section service-settings-panel" aria-label="服务连接">

    {desktop && <section className="service-mode-grid" aria-label="服务模式">
      <button type="button" className={mode === 'local' ? 'is-selected' : ''} onClick={() => void activateLocal()} disabled={busy}>
        <HardDrives size={28} weight="duotone" /><span><small>此设备</small><strong>本地服务</strong><p>iMail 服务直接运行于应用内，不开放本地 HTTP 端口。</p></span>{mode === 'local' && <CheckCircle size={20} weight="fill" />}
      </button>
      <button type="button" className={mode === 'remote' ? 'is-selected' : ''} onClick={onEditRemote} disabled={busy}>
        <Cloud size={28} weight="duotone" /><span><small>多设备共享</small><strong>远程服务</strong><p>连接你部署的服务实例，多台设备使用同一份数据。</p></span>{mode === 'remote' && <CheckCircle size={20} weight="fill" />}
      </button>
    </section>}

    <section className={`service-endpoint-card ${error ? 'has-error' : ''}`}>
      {busy ? <SpinnerGap className="service-spin" size={28} /> : error ? <WarningCircle size={28} weight="duotone" /> : <HardDrives size={28} weight="duotone" />}
      <div><small>{embeddedLocal && mode === 'local' ? '当前服务形态' : '当前服务地址'}</small><strong>{embeddedLocal && mode === 'local' ? '进程内 Rust · 无 HTTP' : address}</strong>
        {info ? <p>实例 {info.instanceId.slice(0, 8)} · 服务 {info.version} · 协议 v{info.protocolVersion}</p> : <p>{error || '正在检查服务身份…'}</p>}
      </div>
      <AppButton appearance="subtle" type="button" onClick={() => void inspect()} disabled={busy}>重新检查</AppButton>
    </section>

    {!desktop && <div className="settings-link-list"><SettingsLinkRow icon={<Cloud size={20} />} title="远程服务" detail="修改并验证当前 iMail 服务地址。" value={address} onClick={onEditRemote} /></div>}

    {desktop && <div className="service-local-lifecycle"><p className="service-rollout-note">{localServiceNote(mode)}</p>
      <div className="service-lifecycle-actions">
        <AppButton appearance="subtle" icon={<FolderOpen size={16} />} onClick={() => void desktopOpenAppLogs().catch((reason) => setError(serviceErrorMessage(reason, '打开应用日志失败')))} disabled={busy}>应用日志</AppButton>
      </div>
    </div>}
  </section>;
}
