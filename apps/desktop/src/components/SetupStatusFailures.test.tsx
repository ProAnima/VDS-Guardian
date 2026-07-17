import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SetupStatusPanel } from "./SetupStatusPanel";
import { createTranslator } from "../shared/preferences";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({
  getSigningIdentityStatus: vi.fn(),
  listCapturePlans: vi.fn(),
  listRepositories: vi.fn(),
  listSshProfiles: vi.fn(),
}));

vi.mock("../shared/commands", async (importOriginal) => ({
  ...await importOriginal<typeof import("../shared/commands")>(),
  ...commands,
}));

describe("SetupStatusPanel failures", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    commands.listCapturePlans.mockResolvedValue([]);
    commands.listRepositories.mockResolvedValue([]);
    commands.listSshProfiles.mockResolvedValue([]);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });

  it("shows the typed remediation returned by a failed prerequisite command", async () => {
    commands.getSigningIdentityStatus.mockRejectedValue({
      code: "signing_storage_unavailable",
      message: "Signing status could not be read.",
      remediation: "Unlock the credential store and retry.",
    });

    await act(async () => root.render(<SetupStatusPanel resourcesRevision={0} t={createTranslator("ru")} />));

    await vi.waitFor(() => expect(container.textContent).toContain(
      "Signing status could not be read. Unlock the credential store and retry.",
    ));
  });

  it("does not expose details from an unknown rejection payload", async () => {
    commands.getSigningIdentityStatus.mockRejectedValue(new Error("internal C:/secret/path"));

    await act(async () => root.render(<SetupStatusPanel resourcesRevision={0} t={createTranslator("ru")} />));

    await vi.waitFor(() => expect(container.textContent).toContain(
      "Повторите проверку; если ошибка сохранится, откройте диагностику.",
    ));
    expect(container.textContent).not.toContain("C:/secret/path");
  });
});
