import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import './theme.css';
import './styles.css';
import { AuthGate } from './features/auth';
import { AppThemeProvider } from './features/appearance';
import { PlatformProvider } from './platform/runtime';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PlatformProvider>
      <AppThemeProvider>
        <AuthGate><App /></AuthGate>
      </AppThemeProvider>
    </PlatformProvider>
  </StrictMode>,
);
