import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { createLightTheme, FluentProvider, type BrandVariants } from '@fluentui/react-components';
import App from './App';
import './styles.css';

const imailBrand: BrandVariants = {
  10: '#061b17', 20: '#0b2b24', 30: '#0d4035', 40: '#105646',
  50: '#126c58', 60: '#14826a', 70: '#168f78', 80: '#2aa089',
  90: '#45b09b', 100: '#64c0ad', 110: '#83cfbe', 120: '#a2ddcf',
  130: '#c0e9df', 140: '#d9f2ec', 150: '#edf8f5', 160: '#f8fcfb',
};

const imailTheme = createLightTheme(imailBrand);
imailTheme.fontFamilyBase = "'Segoe UI Variable Text', 'Segoe UI', 'Microsoft YaHei', sans-serif";

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <FluentProvider theme={imailTheme} className="fluent-root">
      <App />
    </FluentProvider>
  </StrictMode>,
);
