import { Globe, Laptop, Moon, Sun } from "lucide-react";
import { locales, type LocaleId } from "../i18n";
import { themeIds, type ThemeId } from "../shared/preferences";
import { tip } from "../shared/tip";
import type { Preferences } from "../shared/usePreferences";

const themeView: Record<ThemeId, { icon: typeof Sun; label: "themeSystem" | "themeLight" | "themeDark" }> = {
  system: { icon: Laptop, label: "themeSystem" },
  light: { icon: Sun, label: "themeLight" },
  dark: { icon: Moon, label: "themeDark" },
};

export function AppHeader({ preferences }: { preferences: Preferences }) {
  const { locale, setLocale, theme, setTheme, t } = preferences;
  const next = themeIds[(themeIds.indexOf(theme) + 1) % themeIds.length] ?? "system";
  const { icon: ThemeIcon, label } = themeView[theme];
  return (
    <header className="topbar">
      <label className="locale-control" data-tip={t("language")} data-tip-side="stop">
        <Globe size={14} aria-hidden="true" />
        <select value={locale} onChange={(event) => setLocale(event.target.value as LocaleId)} aria-label={t("language")}>
          {locales.map((option) => <option key={option.id} value={option.id}>{option.label}</option>)}
        </select>
      </label>
      <button className="icon-button icon-button--large" type="button" data-tip-side="stop" onClick={() => setTheme(next)} {...tip(`${t("theme")}: ${t(label)}`)}>
        <ThemeIcon size={16} aria-hidden="true" />
      </button>
    </header>
  );
}
