import { lazy, StrictMode, Suspense } from 'preact/compat';
import { createRoot } from 'preact/compat/client';
import App from './App';
import './theme.css';
import './styles.css';
import { AuthGate } from './features/auth';
import { AppThemeProvider } from './features/appearance';
import { PlatformProvider } from './platform/runtime';
import { registerWebServiceWorker } from './service-worker-registration';
import { DesktopFrame } from './components/DesktopFrame';
import { AppErrorBoundary } from './components/ErrorBoundary';
import { desktopLog, installDesktopLogging } from './desktop-logging';
import { preventBrowserRefresh } from './features/shortcuts';

const TrayMenuApp = lazy(() => import('./features/tray-menu/TrayMenuApp').then((module) => ({ default: module.TrayMenuApp })));

installDesktopLogging();
registerWebServiceWorker();
window.addEventListener('keydown', (event) => {
  preventBrowserRefresh(event);
}, { capture: true });

const trayMenu = new URLSearchParams(window.location.search).has('tray-menu');
const root = createRoot(document.getElementById('root')!);
root.render(trayMenu
  ? <StrictMode><Suspense fallback={null}><TrayMenuApp /></Suspense></StrictMode>
  : <StrictMode>
      <DesktopFrame><AppErrorBoundary>
        <PlatformProvider>
          <AppThemeProvider><AuthGate><App /></AuthGate></AppThemeProvider>
        </PlatformProvider>
      </AppErrorBoundary></DesktopFrame>
    </StrictMode>);
void desktopLog('info', 'frontend.rendered', 'Preact compatibility root rendered');
