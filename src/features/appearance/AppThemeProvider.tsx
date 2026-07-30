import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from 'react';
import { FluentProvider } from '@fluentui/react-components';
import type { AppThemeId, CustomThemeDefinition } from '../../app-model';
import { appThemes } from '../../theme';
import { createCustomFluentTheme, customThemeCssVariables } from './theme-runtime';
import { customThemeStorageKey, defaultCustomTheme, defaultThemeId, normalizeCustomTheme, normalizeThemeId, themeStorageKey } from './theme-model';

type ThemeContextValue = {
  themeId: AppThemeId;
  customTheme: CustomThemeDefinition;
  setTheme: (themeId: AppThemeId, customTheme: CustomThemeDefinition) => void;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

function readInitialTheme() {
  if (typeof localStorage === 'undefined') return defaultThemeId;
  return normalizeThemeId(localStorage.getItem(themeStorageKey));
}

function readInitialCustomTheme() {
  if (typeof localStorage === 'undefined') return defaultCustomTheme;
  try { return normalizeCustomTheme(JSON.parse(localStorage.getItem(customThemeStorageKey) ?? '{}')); }
  catch { return defaultCustomTheme; }
}

export function AppThemeProvider({ children }: { children: ReactNode }) {
  const [themeId, setThemeState] = useState<AppThemeId>(readInitialTheme);
  const [customTheme, setCustomTheme] = useState<CustomThemeDefinition>(readInitialCustomTheme);

  const setTheme = (next: AppThemeId, nextCustomTheme: CustomThemeDefinition) => {
    const normalized = normalizeThemeId(next);
    const normalizedCustomTheme = normalizeCustomTheme(nextCustomTheme);
    setThemeState(normalized);
    setCustomTheme(normalizedCustomTheme);
    localStorage.setItem(themeStorageKey, normalized);
    localStorage.setItem(customThemeStorageKey, JSON.stringify(normalizedCustomTheme));
  };

  useEffect(() => {
    const customVariables = customThemeCssVariables(customTheme);
    document.documentElement.dataset.theme = themeId;
    document.documentElement.dataset.customShadow = customTheme.shadow;
    document.documentElement.style.colorScheme = 'light';
    Object.entries(customVariables).forEach(([name, value]) => {
      if (themeId === 'custom') document.documentElement.style.setProperty(name, String(value));
      else document.documentElement.style.removeProperty(name);
    });
  }, [customTheme, themeId]);

  const fluentTheme = useMemo(() => themeId === 'custom' ? createCustomFluentTheme(customTheme) : appThemes[themeId], [customTheme, themeId]);
  const providerStyle = useMemo(() => themeId === 'custom' ? customThemeCssVariables(customTheme) : undefined, [customTheme, themeId]);
  const value = useMemo(() => ({ themeId, customTheme, setTheme }), [customTheme, themeId]);
  return <ThemeContext.Provider value={value}>
    <FluentProvider theme={fluentTheme} style={providerStyle} className="fluent-root" data-theme={themeId}>
      {children}
    </FluentProvider>
  </ThemeContext.Provider>;
}

export function useAppTheme() {
  const context = useContext(ThemeContext);
  if (!context) throw new Error('useAppTheme 必须在 AppThemeProvider 中使用');
  return context;
}
