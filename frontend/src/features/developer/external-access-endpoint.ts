import { useEffect, useState } from 'react';
import { absoluteServiceUrl, configuredServiceMode } from '../../service-config';
import { isTauriRuntime } from '../../platform/tauri-runtime';

type ExternalHttpEndpoint = { baseUrl: string };
type DesktopInvoker = (command: string) => Promise<ExternalHttpEndpoint>;

let embeddedEndpoint: Promise<string> | undefined;

async function tauriInvoke<T>(command: string) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command);
}

export function resolveExternalAccessBaseUrl(options: {
  desktop?: boolean;
  mode?: 'local' | 'remote';
  invoker?: DesktopInvoker;
} = {}) {
  const desktop = options.desktop ?? isTauriRuntime();
  const mode = options.mode ?? configuredServiceMode();
  if (!desktop || mode !== 'local') return Promise.resolve(absoluteServiceUrl(''));
  if (options.invoker) {
    return options.invoker('desktop_start_external_http').then((result) => result.baseUrl);
  }
  embeddedEndpoint ??= tauriInvoke<ExternalHttpEndpoint>('desktop_start_external_http')
    .then((result) => result.baseUrl)
    .catch((error) => {
      embeddedEndpoint = undefined;
      throw error;
    });
  return embeddedEndpoint;
}

export function useExternalAccessBaseUrl() {
  const [baseUrl, setBaseUrl] = useState(() => configuredServiceMode() === 'remote' ? absoluteServiceUrl('') : '');
  const [error, setError] = useState('');

  useEffect(() => {
    let active = true;
    void resolveExternalAccessBaseUrl()
      .then((value) => { if (active) setBaseUrl(value); })
      .catch((value) => { if (active) setError(value instanceof Error ? value.message : String(value)); });
    return () => { active = false; };
  }, []);

  return { baseUrl, error };
}
