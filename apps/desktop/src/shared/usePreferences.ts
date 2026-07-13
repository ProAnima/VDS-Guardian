import { useEffect, useMemo, useState } from "react";
import type { LocaleId } from "../i18n";
import {
  applyDocumentPreferences,
  createTranslator,
  getInitialLocale,
  readTheme,
  resolveTheme,
  storageKeys,
  type ThemeId,
} from "./preferences";

export function usePreferences() {
  const [locale, setLocale] = useState<LocaleId>(getInitialLocale);
  const [theme, setTheme] = useState<ThemeId>(() => readTheme(localStorage.getItem(storageKeys.theme)));
  const [systemDark, setSystemDark] = useState(() => matchMedia("(prefers-color-scheme: dark)").matches);
  const resolvedTheme = resolveTheme(theme, systemDark);

  useEffect(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const update = (event: MediaQueryListEvent) => setSystemDark(event.matches);
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  useEffect(() => {
    applyDocumentPreferences(locale, resolvedTheme);
    localStorage.setItem(storageKeys.locale, locale);
    localStorage.setItem(storageKeys.theme, theme);
  }, [locale, resolvedTheme, theme]);

  return useMemo(() => ({
    locale, setLocale, theme, setTheme, resolvedTheme, t: createTranslator(locale),
  }), [locale, resolvedTheme, theme]);
}

export type Preferences = ReturnType<typeof usePreferences>;
