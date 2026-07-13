import { Laptop, Moon, Sun } from "lucide-react";
import { locales, type LocaleId } from "../i18n";
import type { ThemeId } from "../shared/preferences";
import type { Preferences } from "../shared/usePreferences";

const themes: Array<{ id: ThemeId; icon: typeof Sun; label: "themeSystem" | "themeLight" | "themeDark" }> = [
  { id: "system", icon: Laptop, label: "themeSystem" },
  { id: "light", icon: Sun, label: "themeLight" },
  { id: "dark", icon: Moon, label: "themeDark" },
];

export function AppHeader({ preferences, version }: { preferences: Preferences; version: string }) {
  const { locale, setLocale, theme, setTheme, t } = preferences;
  return (
    <header className="topbar">
      <div className="topbar__identity">
        <span className="topbar__status"><i aria-hidden="true" />{t("iterationBadge")}</span>
        <span className="topbar__version">v{version}</span>
      </div>
      <div className="topbar__controls">
        <label className="locale-control">
          <span>{t("language")}</span>
          <select value={locale} onChange={(event) => setLocale(event.target.value as LocaleId)} aria-label={t("language")}>
            {locales.map((option) => <option key={option.id} value={option.id}>{option.label}</option>)}
          </select>
        </label>
        <div className="theme-control" role="group" aria-label={t("theme")}>
          {themes.map(({ id, icon: Icon, label }) => (
            <button key={id} type="button" data-active={theme === id || undefined} onClick={() => setTheme(id)} aria-label={t(label)} title={t(label)}>
              <Icon size={16} aria-hidden="true" />
            </button>
          ))}
        </div>
      </div>
    </header>
  );
}
