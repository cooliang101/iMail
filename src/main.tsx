import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { FluentProvider } from '@fluentui/react-components';
import App from './App';
import { imailTheme } from './theme';
import './theme.css';
import './styles.css';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <FluentProvider theme={imailTheme} className="fluent-root" data-theme="imail-light">
      <App />
    </FluentProvider>
  </StrictMode>,
);
