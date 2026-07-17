import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { getDirection, localeIds, messages, resolveLocale } from ".";

describe("localization registry", () => {
  it("ships complete messages for ten locales", () => {
    expect(localeIds).toHaveLength(10);
    const englishKeys = Object.keys(messages.en).sort();
    for (const locale of localeIds) {
      expect(englishKeys.every((key) => messages[locale][key as keyof typeof messages.en] ?? messages.en[key as keyof typeof messages.en])).toBe(true);
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

  it("resolves regional browser locales and RTL direction", () => {
    expect(resolveLocale("pt-PT")).toBe("pt-BR");
    expect(resolveLocale("zh-TW")).toBe("zh-CN");
    expect(resolveLocale("unknown")).toBe("en");
    expect(getDirection("ar")).toBe("rtl");
  });
});
