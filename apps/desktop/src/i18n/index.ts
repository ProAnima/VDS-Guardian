import { de, en, es, fr, ru, type MessageKey, type Messages } from "./messages-primary";
import { ar, ja, ko, ptBr, zhCn } from "./messages-secondary";

export const localeIds = ["en", "ru", "de", "fr", "es", "pt-BR", "zh-CN", "ja", "ko", "ar"] as const;
export type LocaleId = (typeof localeIds)[number];

export interface LocaleOption {
  id: LocaleId;
  label: string;
  direction: "ltr" | "rtl";
}

export const locales: readonly LocaleOption[] = [
  { id: "en", label: "English", direction: "ltr" },
  { id: "ru", label: "Русский", direction: "ltr" },
  { id: "de", label: "Deutsch", direction: "ltr" },
  { id: "fr", label: "Français", direction: "ltr" },
  { id: "es", label: "Español", direction: "ltr" },
  { id: "pt-BR", label: "Português", direction: "ltr" },
  { id: "zh-CN", label: "简体中文", direction: "ltr" },
  { id: "ja", label: "日本語", direction: "ltr" },
  { id: "ko", label: "한국어", direction: "ltr" },
  { id: "ar", label: "العربية", direction: "rtl" },
] as const;

export const messages: Record<LocaleId, Messages> = {
  en, ru, de, fr, es, "pt-BR": ptBr, "zh-CN": zhCn, ja, ko, ar,
};

export function isLocale(value: string): value is LocaleId {
  return localeIds.some((locale) => locale === value);
}

export function resolveLocale(value: string | null | undefined): LocaleId {
  if (value && isLocale(value)) return value;
  const base = value?.split("-")[0];
  return localeIds.find((locale) => locale.split("-")[0] === base) ?? "en";
}

export function getDirection(locale: LocaleId): "ltr" | "rtl" {
  return locales.find((option) => option.id === locale)?.direction ?? "ltr";
}

export type Translate = (key: MessageKey) => string;
