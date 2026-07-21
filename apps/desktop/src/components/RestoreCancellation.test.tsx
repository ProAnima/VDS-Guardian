import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RestorePanel } from "./RestorePanel";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({
  cancelJob: vi.fn(),
  executeDeploy: vi.fn(),
  inspectRestoreBackup: vi.fn(),
  listBackups: vi.fn(),
  listRepositories: vi.fn(),
  listSshProfiles: vi.fn(),
  previewDeploy: vi.fn(),
  previewSourceReplacement: vi.fn(),
  executeSourceReplacement: vi.fn(),
}));

vi.mock("../shared/commands", async (importOriginal) => ({
  ...await importOriginal<typeof import("../shared/commands")>(),
  ...commands,
  hasTauriRuntime: () => true,
}));

describe("restore cancellation", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    commands.listRepositories.mockResolvedValue([
      { repositoryId: "repo-1", label: "Archive", path: "D:/archive", recoveryReady: true },
    ]);
    commands.listBackups.mockResolvedValue([
      { backupId: "backup-1", sealedAt: "2026-07-17T00:00:00Z", verification: "verified" },
    ]);
    commands.listSshProfiles.mockResolvedValue([
      { profileId: "profile-1", label: "Source", host: "vds.example", port: 22, user: "root" },
    ]);
    commands.inspectRestoreBackup.mockResolvedValue({
      backupId: "backup-1", sourceProfileId: "profile-1", roots: ["/srv/app"],
      dockerWorkloads: [], entries: [], totalEntries: 0, replacementAvailable: true,
    });
    commands.previewDeploy.mockResolvedValue({
      backupId: "backup-1",
      targetProfileId: "profile-1", targetProfileLabel: "Source", targetPath: "/srv/restored",
      filesystemPayload: "payload/filesystem.tar.zst.enc",
      confirmation: "DEPLOY backup-1 TO profile-1 AT /srv/restored",
    });
    commands.executeDeploy.mockReturnValue(new Promise(() => undefined));
    commands.cancelJob.mockResolvedValue(true);
    commands.previewSourceReplacement.mockResolvedValue({
      backupId: "backup-1", targetProfileId: "profile-1", root: "/srv/app",
      containers: ["app"], replaces: ["/srv/app"], conflicts: ["container_image_changed:app"],
      safetyBackupRequired: true, serviceStopRequired: true,
      confirmation: "REPLACE backup-1 ON profile-1 AT /srv/app STATE abc123", rollbackPath: "pending",
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });

  it("cancels the exact in-flight restore run", async () => {
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    await vi.waitFor(() => expect(container.querySelector('option[value="backup-1"]')).not.toBeNull());
    await act(async () => change(
      container.querySelector<HTMLInputElement>('input[placeholder="deployTargetPathHint"]'),
      "/srv/restored",
    ));
    await act(async () => container.querySelector("form")?.requestSubmit());
    await vi.waitFor(() => expect(button("restoreExecute")).toBeDefined());
    await act(async () => change(
      container.querySelector<HTMLInputElement>('input[placeholder="restoreConfirmPlaceholder"]'),
      "DEPLOY backup-1 TO profile-1 AT /srv/restored",
    ));
    await act(async () => button("restoreExecute").click());
    await vi.waitFor(() => expect(button("restoreCancelRunning")).toBeDefined());
    const request = commands.executeDeploy.mock.calls[0]?.[0] as { runId?: string };
    expect(request.runId).toMatch(/^[0-9a-f-]{36}$/);
    await act(async () => button("restoreCancelRunning").click());
    expect(commands.cancelJob).toHaveBeenCalledWith(request.runId);
  });

  it("rejects an existing remote destination before confirmation", async () => {
    commands.previewDeploy.mockRejectedValueOnce(new Error("target exists"));
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    await vi.waitFor(() => expect(container.querySelector('option[value="backup-1"]')).not.toBeNull());
    await act(async () => change(
      container.querySelector<HTMLInputElement>('input[placeholder="deployTargetPathHint"]'),
      "/srv/restored",
    ));
    await act(async () => container.querySelector("form")?.requestSubmit());
    await vi.waitFor(() => expect(container.textContent).toContain("restoreErrorFallback"));
    expect(container.textContent).not.toContain("restoreExecute");
    expect(commands.executeDeploy).not.toHaveBeenCalled();
  });

  it("shows live replacement conflicts and keeps execution disabled", async () => {
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    await vi.waitFor(() => expect(container.textContent).toContain("restoreImpactReplaces"));
    await act(async () => button("restoreImpactReplaces").click());
    await act(async () => container.querySelector("form")?.requestSubmit());
    await vi.waitFor(() => expect(container.textContent).toContain("restoreFailureChanged: app"));
    await act(async () => change(
      container.querySelector<HTMLInputElement>('input[placeholder="restoreConfirmPlaceholder"]'),
      "REPLACE backup-1 ON profile-1 AT /srv/app STATE abc123",
    ));
    expect(button("restoreExecute").disabled).toBe(true);
    expect(commands.executeSourceReplacement).not.toHaveBeenCalled();
  });

  function button(label: string): HTMLButtonElement {
    const match = [...container.querySelectorAll("button")]
      .find((candidate) => candidate.textContent?.includes(label));
    if (!match) throw new Error(`missing button: ${label}`);
    return match;
  }
});

function change(input: HTMLInputElement | null, value: string): void {
  if (!input) throw new Error("missing input");
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}
