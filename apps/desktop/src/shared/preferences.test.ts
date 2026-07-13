import { describe, expect, it } from "vitest";
import { readTheme, resolveTheme } from "./preferences";

describe("theme preferences", () => {
  it("resolves the system theme explicitly", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
  });

  it("fails unknown persisted themes back to system", () => {
    expect(readTheme("dark")).toBe("dark");
    expect(readTheme("sepia")).toBe("system");
  });
});
