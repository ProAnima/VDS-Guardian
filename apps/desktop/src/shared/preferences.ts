import { getDirection, messages, resolveLocale, type LocaleId, type Translate } from "../i18n";

export const themeIds = ["system", "light", "dark"] as const;
export type ThemeId = (typeof themeIds)[number];
export type ResolvedTheme = Exclude<ThemeId, "system">;

export const storageKeys = {
  locale: "vds-guardian.locale",
  theme: "vds-guardian.theme",
} as const;

export function resolveTheme(theme: ThemeId, systemDark: boolean): ResolvedTheme {
  return theme === "system" ? (systemDark ? "dark" : "light") : theme;
}

export function readTheme(value: string | null): ThemeId {
  return themeIds.find((theme) => theme === value) ?? "system";
}

export function applyDocumentPreferences(locale: LocaleId, theme: ResolvedTheme): void {
  document.documentElement.lang = locale;
  document.documentElement.dir = getDirection(locale);
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
}

export function createTranslator(locale: LocaleId): Translate {
  return (key) => messages[locale][key] ?? messages.en[key] ?? key;
}

export function getInitialLocale(): LocaleId {
  return resolveLocale(localStorage.getItem(storageKeys.locale) ?? navigator.language);
}
