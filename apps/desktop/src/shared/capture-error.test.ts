import { describe, expect, it } from "vitest";
import { captureErrorText } from "./capture-error";

describe("captureErrorText", () => {
  it("preserves the typed failure remediation returned by the desktop command", () => {
    expect(captureErrorText(
      { code: "capture_cancelled", message: "Capture was cancelled.", remediation: "Run it again." },
      "Fallback",
    )).toBe("Capture was cancelled. Run it again.");
  });

  it("does not expose an unknown error object", () => {
    expect(captureErrorText(new Error("internal detail"), "Fallback")).toBe("Fallback");
  });
});
