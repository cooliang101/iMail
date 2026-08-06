import { useEffect, useState } from 'react';
import { Button } from '@fluentui/react-components';
import { CheckCircle, Cloud, FolderOpen, HardDrives, Pause, SpinnerGap, Trash, WarningCircle } from '@phosphor-icons/react';
import type { ServiceInfo } from '../../types';
import {
  configuredLocalServicePort,
  configuredLocalServiceUrl,
  configuredServiceMode,
  configuredServiceUrl,
  normalizeLocalServicePort,
  saveLocalServiceSuspended,
  saveServiceSelection,
  type ServiceMode,
} from '../../service-config';
import { isTauriRuntime } from '../../platform/tauri-runtime';
import { desktopEnableLocalService, desktopEnableLocalServiceAtPort, desktopLocalServiceStatus, desktopOpenAppLogs, desktopOpenLocalServiceLogs, desktopPauseLocalService, desktopRemoveLocalService, type LocalServiceStatus } from '../../local-service';
import { describeDesktopLogValue, desktopLog } from '../../desktop-logging';
import { ServiceAddressEditor } from './ServiceAddressEditor';
import { LocalPortRecovery, localPortConflictMessage, suggestedLocalServicePort } from './LocalPortRecovery';
import { serviceErrorMessage, testServiceConnection } from './service-connection';
import { suspendManagedLocalService, switchToLocalService, type ServiceTransitionDependencies } from './service-transition';

function announceServiceChange() {
  window.dispatchEvent(new Event('imail:service-changed'));
}

function localServiceNote(mode: ServiceMode, status?: LocalServiceStatus) {
  if (mode === 'remote') {
    return status?.running
      ? '本地守护服务正在停止；远程连接已选中。'
      : '本地服务已暂停并保留数据，当前客户端使用远程服务。';
  }
  if (status?.running) return `用户级守护服务正在运行${status.version ? ` · ${status.version}` : ''}，退出桌面界面后仍会继续同步。`;
  if (status?.state === 'error') {
    const occurred = status.diagnostic?.updatedAtEpochSeconds
      ? new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(status.diagnostic.updatedAtEpochSeconds * 1000))
      : '';
    return `${status.error || '本地守护服务启动失败'}${occurred ? ` · 最近失败 ${occurred}` : ''}`;
  }
  if (status?.enabled) return '用户级守护服务正在启动。';
  if (status?.installed) return '本地服务已暂停；再次选择“本地服务”即可恢复。';
  return '本地服务将在启用后注册为当前用户的后台守护进程。';
}

export function ServicePanel() {
  const desktop = isTauriRuntime();
  const [mode, setMode] = useState<ServiceMode>(configuredServiceMode);
  const [info, setInfo] = useState<ServiceInfo>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [remoteEditorOpen, setRemoteEditorOpen] = useState(false);
  const [localStatus, setLocalStatus] = useState<LocalServiceStatus>();
  const [localPort, setLocalPort] = useState(() => String(configuredLocalServicePort()));
  const [portRecoveryOpen, setPortRecoveryOpen] = useState(false);

  function transitionDependencies(): ServiceTransitionDependencies {
    return {
      desktop,
      currentMode: configuredServiceMode,
      testConnection: testServiceConnection,
      enableLocal: (port) => port === undefined ? desktopEnableLocalService() : desktopEnableLocalServiceAtPort(port),
      pauseLocal: desktopPauseLocalService,
      saveSelection: saveServiceSelection,
    };
  }

  async function inspect(url = configuredServiceUrl()) {
    setError('');
    try { setInfo(await testServiceConnection(url)); }
    catch (reason) {
      setInfo(undefined); setError(serviceErrorMessage(reason, '服务不可用'));
      void desktopLog('warn', 'service.inspect_failed', describeDesktopLogValue(reason));
    }
    if (desktop) await desktopLocalServiceStatus().then((status) => {
      setLocalStatus(status);
      if (status.error && localPortConflictMessage(status.error)) {
        setLocalPort(String(suggestedLocalServicePort(configuredLocalServicePort())));
        setPortRecoveryOpen(true);
      }
    }).catch((reason) => { void desktopLog('warn', 'service.status_failed', describeDesktopLogValue(reason)); });
  }

  useEffect(() => {
    void inspect();
  }, []);

  async function activateLocal(useSelectedPort = false) {
    setRemoteEditorOpen(false);
    setBusy(true); setError('');
    try {
      const port = useSelectedPort ? normalizeLocalServicePort(localPort) : undefined;
      const { status, info: nextInfo } = await switchToLocalService(configuredLocalServiceUrl(), transitionDependencies(), port);
      setLocalStatus(status);
      setMode('local'); setInfo(nextInfo);
      setPortRecoveryOpen(false);
      setRemoteEditorOpen(false);
      announceServiceChange();
    } catch (reason) {
      void desktopLog('error', 'service.activate_failed', describeDesktopLogValue(reason));
      setError(`本地服务尚未就绪：${serviceErrorMessage(reason, '未知错误')}`);
      if (localPortConflictMessage(reason)) {
        if (!portRecoveryOpen) setLocalPort(String(suggestedLocalServicePort(configuredLocalServicePort())));
        setPortRecoveryOpen(true);
      }
    } finally { setBusy(false); }
  }

  async function pauseLocal() {
    setBusy(true); setError('');
    try {
      setLocalStatus(await suspendManagedLocalService({ suspendLocal: desktopPauseLocalService, enableLocal: desktopEnableLocalService, saveSuspended: saveLocalServiceSuspended }));
      setPortRecoveryOpen(false);
      setInfo(undefined); announceServiceChange();
    }
    catch (reason) {
      void desktopLog('error', 'service.pause_failed', describeDesktopLogValue(reason));
      setError(serviceErrorMessage(reason, '暂停本地服务失败'));
    }
    finally { setBusy(false); }
  }

  async function removeLocal() {
    if (!window.confirm('移除本地服务的守护项和运行文件？邮件数据会保留。')) return;
    setBusy(true); setError('');
    try {
      setLocalStatus(await suspendManagedLocalService({ suspendLocal: desktopRemoveLocalService, enableLocal: desktopEnableLocalService, saveSuspended: saveLocalServiceSuspended }));
      setPortRecoveryOpen(false);
      setInfo(undefined); announceServiceChange();
    }
    catch (reason) {
      void desktopLog('error', 'service.remove_failed', describeDesktopLogValue(reason));
      setError(serviceErrorMessage(reason, '移除本地服务失败'));
    }
    finally { setBusy(false); }
  }

  const address = configuredServiceUrl();
  return <section className="settings-feature-panel">
    <header className="settings-panel-heading"><div><span>客户端连接</span><h2>iMail 服务</h2><p>选择使用此设备上的本地服务，或连接用于多设备共享的远程服务。</p></div></header>
    <div className="settings-panel-body service-settings-panel">

    {desktop && <section className="service-mode-grid" aria-label="服务模式">
      <button type="button" className={mode === 'local' ? 'is-selected' : ''} onClick={() => void activateLocal()} disabled={busy}>
        <HardDrives size={28} weight="duotone" /><span><small>此设备</small><strong>本地服务</strong><p>用户级守护服务持续同步，数据保存在当前设备。</p></span>{mode === 'local' && <CheckCircle size={20} weight="fill" />}
      </button>
      <button type="button" className={mode === 'remote' ? 'is-selected' : ''} onClick={() => { setPortRecoveryOpen(false); setRemoteEditorOpen(true); }} disabled={busy}>
        <Cloud size={28} weight="duotone" /><span><small>多设备共享</small><strong>远程服务</strong><p>连接你部署的服务实例，多台设备使用同一份数据。</p></span>{mode === 'remote' && <CheckCircle size={20} weight="fill" />}
      </button>
    </section>}

    <section className={`service-endpoint-card ${error ? 'has-error' : ''}`}>
      {busy ? <SpinnerGap className="service-spin" size={28} /> : error ? <WarningCircle size={28} weight="duotone" /> : <HardDrives size={28} weight="duotone" />}
      <div><small>当前服务地址</small><strong>{address}</strong>
        {info ? <p>实例 {info.instanceId.slice(0, 8)} · 服务 {info.version} · 协议 v{info.protocolVersion}</p> : <p>{error || '正在检查服务身份…'}</p>}
      </div>
      <Button appearance="subtle" type="button" onClick={() => void inspect()} disabled={busy}>重新检查</Button>
    </section>

    {desktop && portRecoveryOpen && <LocalPortRecovery port={localPort} busy={busy} onPortChange={setLocalPort} onRetry={() => void activateLocal(true)} />}

    {(!desktop || mode === 'remote' || remoteEditorOpen) && <ServiceAddressEditor onCancel={remoteEditorOpen && mode !== 'remote' ? () => setRemoteEditorOpen(false) : undefined} onSaved={() => { setMode('remote'); setPortRecoveryOpen(false); setRemoteEditorOpen(false); void inspect(); if (desktop) void desktopLocalServiceStatus().then(setLocalStatus).catch(() => undefined); }} />}
    {desktop && <div className="service-local-lifecycle">{(mode === 'local' || localStatus?.installed || localStatus?.dataPresent) && <p className="service-rollout-note">{localServiceNote(mode, localStatus)}</p>}
      <div className="service-lifecycle-actions">
        <Button appearance="subtle" icon={<FolderOpen size={16} />} onClick={() => void desktopOpenAppLogs().catch((reason) => setError(serviceErrorMessage(reason, '打开应用日志失败')))} disabled={busy}>应用日志</Button>
        {localStatus?.installed && <Button appearance="subtle" icon={<FolderOpen size={16} />} onClick={() => void desktopOpenLocalServiceLogs().catch((reason) => setError(serviceErrorMessage(reason, '打开服务日志失败')))} disabled={busy}>服务日志</Button>}
        {localStatus?.installed && <>
        {localStatus.enabled && <Button appearance="subtle" icon={<Pause size={16} />} onClick={() => void pauseLocal()} disabled={busy}>暂停本地服务</Button>}
        <Button appearance="subtle" icon={<Trash size={16} />} onClick={() => void removeLocal()} disabled={busy}>移除运行文件</Button>
        </>}
      </div>
    </div>}
    </div>
  </section>;
}
