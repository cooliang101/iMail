import type { ServiceInfo } from '../../types';
import { secureRemoteServiceUrl, type ServiceMode, type ServiceSelection } from '../../service-config';

export type ServiceTransitionDependencies = {
  testConnection: (url: string) => Promise<ServiceInfo>;
  saveSelection: (selection: ServiceSelection) => unknown;
};

export type ServiceReadinessDependencies = {
  mode: ServiceMode;
  serviceUrl: string;
  testConnection: (url: string) => Promise<ServiceInfo>;
};

export async function runWithReadySelectedService<T>(
  dependencies: ServiceReadinessDependencies,
  operation: () => Promise<T>,
) {
  if (dependencies.mode === 'remote') secureRemoteServiceUrl(dependencies.serviceUrl);
  await dependencies.testConnection(dependencies.serviceUrl);
  return operation();
}

export async function switchToRemoteService(
  remoteUrl: string,
  dependencies: ServiceTransitionDependencies,
) {
  const secureUrl = secureRemoteServiceUrl(remoteUrl);
  const info = await dependencies.testConnection(secureUrl);
  dependencies.saveSelection({ mode: 'remote', remoteUrl: secureUrl });
  return info;
}

export async function switchToLocalService(
  localUrl: string,
  dependencies: ServiceTransitionDependencies,
) {
  const info = await dependencies.testConnection(localUrl);
  dependencies.saveSelection({ mode: 'local' });
  return info;
}
