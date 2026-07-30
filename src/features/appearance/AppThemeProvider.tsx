import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from 'react';
import { FluentProvider } from '@fluentui/react-components';
import type { AppThemeId } from '../../app-model';
import { appThemes } from '../../theme';
import { defaultThemeId, normalizeThemeId, themeStorageKey } from './theme-model';

type ThemeContextValue = {
  themeId: AppThemeId;
  setThemeId: (themeId: AppThemeId) => void;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

function readInitialTheme() {
  if (typeof localStorage === 'undefined') return defaultThemeId;
  return normalizeThemeId(localStorage.getItem(themeStorageKey));
}

export function AppThemeProvider({ children }: { children: ReactNode }) {
  const [themeId, setThemeState] = useState<AppThemeId>(readInitialTheme);

  const setThemeId = (next: AppThemeId) => {
    const normalized = normalizeThemeId(next);
    setThemeState(normalized);
    localStorage.setItem(themeStorageKey, normalized);
  };

  useEffect(() => {
    document.documentElement.dataset.theme = themeId;
    document.documentElement.style.colorScheme = 'light';
  }, [themeId]);

  const value = useMemo(() => ({ themeId, setThemeId }), [themeId]);
  return <ThemeContext.Provider value={value}>
    <FluentProvider theme={appThemes[themeId]} className="fluent-root" data-theme={themeId}>
      {children}
    </FluentProvider>
  </ThemeContext.Provider>;
}

export function useAppTheme() {
  const context = useContext(ThemeContext);
  if (!context) throw new Error('useAppTheme 必须在 AppThemeProvider 中使用');
  return context;
}
