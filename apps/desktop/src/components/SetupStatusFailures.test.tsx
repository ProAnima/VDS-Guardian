import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SetupStatusPanel } from "./SetupStatusPanel";
import { createTranslator } from "../shared/preferences";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({
  getSigningIdentityStatus: vi.fn(),
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

  it("opens the exact prerequisite that needs attention", async () => {
    const openSettings = vi.fn();
    const manageServers = vi.fn();
    commands.getSigningIdentityStatus.mockResolvedValue({ state: "ready", identity: null });

    await act(async () => root.render(
      <SetupStatusPanel
        onManageServers={manageServers}
        onOpenSettings={openSettings}
        resourcesRevision={0}
        t={createTranslator("ru")}
      />,
    ));

    const buttons = await vi.waitFor(() => {
      const candidates = [...container.querySelectorAll<HTMLButtonElement>(".setup-status__items button")];
      expect(candidates).toHaveLength(2);
      return candidates;
    });
    await act(async () => buttons.find((button) => button.textContent?.includes("Хранилище"))?.click());
    await act(async () => buttons.find((button) => button.textContent?.includes("Сервер"))?.click());
    expect(openSettings).toHaveBeenCalledOnce();
    expect(openSettings).toHaveBeenCalledWith("storage");
    expect(manageServers).toHaveBeenCalledOnce();
  });

  it("collapses completed prerequisites into one ready state", async () => {
    commands.getSigningIdentityStatus.mockResolvedValue({ state: "ready", identity: null });
    commands.listRepositories.mockResolvedValue([
      { repositoryId: "repo", label: "Archive", path: "D:/archive", recoveryReady: true },
    ]);
    commands.listSshProfiles.mockResolvedValue([
      { profileId: "server", label: "VDS", host: "vds.example", port: 22, user: "backup", authKind: "ssh_key" },
    ]);

    await act(async () => root.render(<SetupStatusPanel resourcesRevision={0} t={createTranslator("ru")} />));

    await vi.waitFor(() => expect(container.textContent).toContain("Можно создавать бэкап"));
    expect(container.querySelector(".setup-status__items")).toBeNull();
  });
});
