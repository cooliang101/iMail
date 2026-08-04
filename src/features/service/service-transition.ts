import type { ServiceInfo } from '../../types';
import type { LocalServiceStatus } from '../../local-service';
import { secureRemoteServiceUrl, type ServiceMode, type ServiceSelection } from '../../service-config';

export type ServiceTransitionDependencies = {
  desktop: boolean;
  currentMode: () => ServiceMode;
  testConnection: (url: string) => Promise<ServiceInfo>;
  enableLocal: () => Promise<LocalServiceStatus>;
  pauseLocal: () => Promise<LocalServiceStatus>;
  saveSelection: (selection: ServiceSelection) => unknown;
};

export type ServiceReadinessDependencies = {
  desktop: boolean;
  mode: ServiceMode;
  serviceUrl: string;
  localSuspended: boolean;
  testConnection: (url: string) => Promise<ServiceInfo>;
  enableLocal: () => Promise<LocalServiceStatus>;
};

function transitionError(primary: unknown, rollback: unknown, action: string) {
  const primaryMessage = primary instanceof Error ? primary.message : String(primary);
  const rollbackMessage = rollback instanceof Error ? rollback.message : String(rollback);
  return new Error(`${primaryMessage}；${action}也失败：${rollbackMessage}`, { cause: primary });
}

export async function runWithReadySelectedService<T>(
  dependencies: ServiceReadinessDependencies,
  operation: () => Promise<T>,
) {
  if (dependencies.mode === 'remote') secureRemoteServiceUrl(dependencies.serviceUrl);
  if (dependencies.desktop && dependencies.mode === 'local') {
    if (dependencies.localSuspended) {
      throw new Error('本地服务已暂停或移除，请在“服务连接”中重新启用');
    }
    const status = await dependencies.enableLocal();
    if (!status.running) throw new Error(status.error || '本地守护服务尚未就绪');
  }
  await dependencies.testConnection(dependencies.serviceUrl);
  return operation();
}

export type LocalSuspensionDependencies = {
  suspendLocal: () => Promise<LocalServiceStatus>;
  enableLocal: () => Promise<LocalServiceStatus>;
  saveSuspended: (suspended: boolean) => unknown;
};

export async function suspendManagedLocalService(dependencies: LocalSuspensionDependencies) {
  const status = await dependencies.suspendLocal();
  try {
    dependencies.saveSuspended(true);
    return status;
  } catch (error) {
    try {
      await dependencies.enableLocal();
    } catch (rollbackError) {
      throw transitionError(error, rollbackError, '恢复本地服务');
    }
    throw error;
  }
}

export async function switchToRemoteService(
  remoteUrl: string,
  dependencies: ServiceTransitionDependencies,
) {
  const secureUrl = secureRemoteServiceUrl(remoteUrl);
  const previousMode = dependencies.currentMode();
  const info = await dependencies.testConnection(secureUrl);
  let localPauseAttempted = false;

  try {
    if (dependencies.desktop) {
      localPauseAttempted = true;
      await dependencies.pauseLocal();
    }
    dependencies.saveSelection({ mode: 'remote', remoteUrl: secureUrl });
    return info;
  } catch (error) {
    if (localPauseAttempted && previousMode === 'local') {
      try {
        await dependencies.enableLocal();
      } catch (rollbackError) {
        throw transitionError(error, rollbackError, '恢复原本地服务');
      }
    }
    throw error;
  }
}

export async function switchToLocalService(
  localUrl: string,
  dependencies: ServiceTransitionDependencies,
) {
  const previousMode = dependencies.currentMode();
  let localEnableAttempted = false;

  try {
    localEnableAttempted = true;
    const status = await dependencies.enableLocal();
    if (!status.running) throw new Error(status.error || '本地守护服务尚未就绪');
    const info = await dependencies.testConnection(localUrl);
    dependencies.saveSelection({ mode: 'local' });
    return { info, status };
  } catch (error) {
    if (localEnableAttempted && previousMode === 'remote') {
      try {
        await dependencies.pauseLocal();
      } catch (rollbackError) {
        throw transitionError(error, rollbackError, '恢复原远程模式');
      }
    }
    throw error;
  }
}
