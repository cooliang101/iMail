import { lazy, StrictMode, Suspense } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import './theme.css';
import './styles.css';
import { AuthGate } from './features/auth';
import { AppThemeProvider } from './features/appearance';
import { PlatformProvider } from './platform/runtime';
import { registerWebServiceWorker } from './service-worker-registration';
import { DesktopFrame } from './components/DesktopFrame';
import { desktopLog, installDesktopLogging } from './desktop-logging';

const TrayMenuApp = lazy(() => import('./features/tray-menu/TrayMenuApp').then((module) => ({ default: module.TrayMenuApp })));

installDesktopLogging();
registerWebServiceWorker();

const trayMenu = new URLSearchParams(window.location.search).has('tray-menu');
createRoot(document.getElementById('root')!).render(trayMenu
  ? <StrictMode><Suspense fallback={null}><TrayMenuApp /></Suspense></StrictMode>
  : <StrictMode>
      <PlatformProvider>
        <AppThemeProvider>
          <DesktopFrame><AuthGate><App /></AuthGate></DesktopFrame>
        </AppThemeProvider>
      </PlatformProvider>
    </StrictMode>);
void desktopLog('info', 'frontend.rendered', 'React root rendered');
