import { describe, expect, it } from "vitest";
import { newRunId } from "./run-id";

describe("newRunId", () => {
  it("creates a UUIDv7-shaped correlation identifier", () => {
    expect(newRunId()).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  });
});
