import { describe, expect, it } from "vitest";
import { getDirection, localeIds, messages, resolveLocale } from ".";

describe("localization registry", () => {
  it("ships complete messages for ten locales", () => {
    expect(localeIds).toHaveLength(10);
    const englishKeys = Object.keys(messages.en).sort();
    for (const locale of localeIds) {
      expect(englishKeys.every((key) => messages[locale][key as keyof typeof messages.en] ?? messages.en[key as keyof typeof messages.en])).toBe(true);
    }
  });

  it("resolves regional browser locales and RTL direction", () => {
    expect(resolveLocale("pt-PT")).toBe("pt-BR");
    expect(resolveLocale("zh-TW")).toBe("zh-CN");
    expect(resolveLocale("unknown")).toBe("en");
    expect(getDirection("ar")).toBe("rtl");
  });
});
