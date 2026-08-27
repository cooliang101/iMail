import { createContext, type ComponentChildren } from 'preact';
import { useCallback, useContext, useEffect, useMemo, useState } from 'preact/compat';
import type { AppLanguage } from '../../app-model';
import { englishMessages } from './messages';

export const languageStorageKey = 'imail.language';
const languageEvent = 'imail:language-change';

export function normalizeLanguage(value: unknown): AppLanguage {
  return value === 'en-US' ? 'en-US' : 'zh-CN';
}

export function setAppLanguage(language: AppLanguage) {
  try { localStorage.setItem(languageStorageKey, language); } catch { /* The preference API remains authoritative. */ }
  document.documentElement.lang = language;
  window.dispatchEvent(new CustomEvent<AppLanguage>(languageEvent, { detail: language }));
}

function initialLanguage(): AppLanguage {
  try {
    const stored = localStorage.getItem(languageStorageKey);
    if (stored) return normalizeLanguage(stored);
  } catch { /* Fall through to the browser locale. */ }
  return navigator.language.toLowerCase().startsWith('en') ? 'en-US' : 'zh-CN';
}

type Variables = Record<string, string | number>;
type I18nValue = { language: AppLanguage; t: (message: string, variables?: Variables) => string };
const I18nContext = createContext<I18nValue>({ language: 'zh-CN', t: (message) => message });

export function I18nProvider({ children }: { children: ComponentChildren }) {
  const [language, setLanguage] = useState<AppLanguage>(initialLanguage);
  useEffect(() => {
    document.documentElement.lang = language;
    const listener = (event: Event) => setLanguage(normalizeLanguage((event as CustomEvent<AppLanguage>).detail));
    window.addEventListener(languageEvent, listener);
    return () => window.removeEventListener(languageEvent, listener);
  }, [language]);
  const t = useCallback((message: string, variables: Variables = {}) => {
    const template = language === 'en-US' ? englishMessages[message] ?? message : message;
    return template.replace(/\{(\w+)\}/g, (_, key: string) => String(variables[key] ?? `{${key}}`));
  }, [language]);
  const value = useMemo(() => ({ language, t }), [language, t]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() { return useContext(I18nContext); }
