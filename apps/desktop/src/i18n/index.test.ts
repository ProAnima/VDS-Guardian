import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { getDirection, localeIds, messages, resolveLocale } from ".";

describe("localization registry", () => {
  it("ships ten complete locales without fallback gaps", () => {
    expect(localeIds).toHaveLength(10);
    const englishKeys = Object.keys(messages.en).sort();
    for (const locale of localeIds) {
      expect(Object.keys(messages[locale]).sort(), locale).toEqual(englishKeys);
      expect(Object.values(messages[locale]).every((value) => value.trim().length > 0), locale).toBe(true);
    }
  });

  it("keeps Russian complete and production components free of hard-coded Cyrillic copy", () => {
    expect(Object.keys(messages.ru).sort()).toEqual(Object.keys(messages.en).sort());
    const components = join(process.cwd(), "src", "components");
    const files = readdirSync(components)
      .filter((name) => name.endsWith(".tsx") && !name.includes(".test."));
    for (const file of files) {
      expect(readFileSync(join(components, file), "utf8"), file).not.toMatch(/[А-Яа-яЁё]/u);
    }
  });

  it("does not leak Russian or long English fallback copy into other locales", () => {
    for (const locale of localeIds.filter((id) => id !== "en" && id !== "ru")) {
      const localized = messages[locale];
      expect(Object.values(localized).join(" "), locale).not.toMatch(/[А-Яа-яЁё]/u);
      for (const key of Object.keys(messages.en) as Array<keyof typeof messages.en>) {
        if (messages.en[key].length > 20) expect(localized[key], `${locale}.${key}`).not.toBe(messages.en[key]);
      }
    }
  });

  it("resolves regional browser locales and RTL direction", () => {
    expect(resolveLocale("ru-RU")).toBe("ru");
    expect(resolveLocale("pt-PT")).toBe("pt-BR");
    expect(resolveLocale("zh-TW")).toBe("zh-CN");
    expect(resolveLocale("unknown")).toBe("en");
    expect(getDirection("ar")).toBe("rtl");
  });
});
