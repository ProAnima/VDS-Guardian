import { describe, expect, it } from "vitest";
import { evaluateSetupReadiness } from "./setup-readiness";
import { createTranslator } from "../shared/preferences";

describe("evaluateSetupReadiness", () => {
  it("keeps a repository with missing recovery material actionable", () => {
    const items = evaluateSetupReadiness({
      identity: { state: "ready", identity: null },
      repositories: [{ repositoryId: "repo", label: "Archive", path: "D:/archive", recoveryReady: false }],
      profiles: [{ profileId: "server", label: "VDS", host: "vds.example", port: 22, user: "backup" }],
      plans: [{ planId: "plan", profileId: "server", repositoryId: "repo", roots: ["/srv/app"] }],
    }, createTranslator("ru"));

    expect(items.find((item) => item.label === "Хранилище и recovery")).toMatchObject({ readiness: "attention", detail: "Recovery готово: 0/1." });
  });
});
