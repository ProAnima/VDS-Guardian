import { describe, expect, it } from "vitest";
import { getFoundationStatus } from "./commands";

describe("foundation bridge", () => {
  it("keeps live operations disabled in browser preview", async () => {
    const status = await getFoundationStatus();
    expect(status.liveOperationsEnabled).toBe(false);
    expect(status.iteration).toContain("Iteration 0");
  });
});

