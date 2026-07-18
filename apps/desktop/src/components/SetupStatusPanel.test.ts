import { describe, expect, it } from "vitest";
import { evaluateSetupReadiness } from "./setup-readiness";
import { createTranslator } from "../shared/preferences";

describe("evaluateSetupReadiness", () => {
  it("keeps a repository with missing recovery material actionable", () => {
    const items = evaluateSetupReadiness({
      identity: { state: "ready", identity: null },
      repositories: [{ repositoryId: "repo", label: "Archive", path: "D:/archive", recoveryReady: false }],
      profiles: [{ profileId: "server", label: "VDS", host: "vds.example", port: 22, user: "backup" }],
    }, createTranslator("ru"));

    expect(items.find((item) => item.label === "Хранилище бэкапов")).toMatchObject({ readiness: "attention", detail: "Ключ восстановления готов: 0/1." });
  });

  it("does not require a saved plan before the visual backup flow", () => {
    const items = evaluateSetupReadiness({
      identity: { state: "ready", identity: null },
      repositories: [{ repositoryId: "repo", label: "Archive", path: "D:/archive", recoveryReady: true }],
      profiles: [{ profileId: "server", label: "VDS", host: "vds.example", port: 22, user: "backup" }],
    }, createTranslator("en"));

    expect(items).toHaveLength(3);
    expect(items.every((item) => item.readiness === "ready")).toBe(true);
  });
});
