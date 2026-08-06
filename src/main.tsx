import { StrictMode } from 'react';
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

installDesktopLogging();
registerWebServiceWorker();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PlatformProvider>
      <AppThemeProvider>
        <DesktopFrame><AuthGate><App /></AuthGate></DesktopFrame>
      </AppThemeProvider>
    </PlatformProvider>
  </StrictMode>,
);
void desktopLog('info', 'frontend.rendered', 'React root rendered');
