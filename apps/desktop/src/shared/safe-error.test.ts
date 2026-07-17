import { describe, expect, it } from "vitest";
import { safeErrorText } from "./safe-error";

describe("safeErrorText", () => {
  it("preserves a bounded typed failure and its remediation", () => {
    expect(safeErrorText({
      code: "profile_storage_unavailable",
      message: "The profile store could not be read.",
      remediation: "Check local application storage and try again.",
    }, "Fallback")).toBe(
      "The profile store could not be read. Check local application storage and try again.",
    );
  });

  it.each([
    new Error("internal path: C:/secret"),
    { code: "INVALID CODE", message: "Public", remediation: "Retry" },
    { code: "internal_error", message: "", remediation: "Retry" },
    { code: "internal_error", message: "Public", remediation: "x".repeat(1025) },
    { message: "Public", remediation: "Retry" },
  ])("does not expose an unknown or malformed failure", (error) => {
    expect(safeErrorText(error, "Fallback")).toBe("Fallback");
  });
});
