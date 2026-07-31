import { isTauriRuntime } from './platform/tauri-runtime';

type RegistrationEnvironment = {
  production: boolean;
  tauri: boolean;
  protocol: string;
  supported: boolean;
};

export function shouldRegisterServiceWorker(environment: RegistrationEnvironment) {
  return environment.production
    && !environment.tauri
    && environment.supported
    && ['http:', 'https:'].includes(environment.protocol);
}

export function registerWebServiceWorker() {
  if (!shouldRegisterServiceWorker({
    production: import.meta.env.PROD,
    tauri: isTauriRuntime(),
    protocol: window.location.protocol,
    supported: 'serviceWorker' in navigator,
  })) return;

  window.addEventListener('load', () => {
    const base = import.meta.env.BASE_URL.endsWith('/') ? import.meta.env.BASE_URL : `${import.meta.env.BASE_URL}/`;
    const scriptUrl = new URL(`${base}sw.js`, window.location.origin);
    void navigator.serviceWorker.register(scriptUrl, { scope: base, updateViaCache: 'none' }).catch((error) => {
      console.warn('[service-worker] registration failed', error);
    });
  }, { once: true });
}
