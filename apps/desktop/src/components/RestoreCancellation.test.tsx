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
      { profileId: "profile-1", label: "Source", host: "vds.example", port: 22, user: "root", authKind: "ssh_key" },
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
    await vi.waitFor(() => expect(container.querySelector('[data-backup-id="backup-1"]')).not.toBeNull());
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
    expect(button("restoreCancelRunning").disabled).toBe(true);
    await act(async () => button("restoreCancelRunning").click());
    expect(commands.cancelJob).toHaveBeenCalledTimes(1);
  });

  it("rejects an existing remote destination before confirmation", async () => {
    commands.previewDeploy.mockRejectedValueOnce(new Error("target exists"));
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    await vi.waitFor(() => expect(container.querySelector('[data-backup-id="backup-1"]')).not.toBeNull());
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
    await vi.waitFor(() => expect(container.querySelector('[aria-label="restoreModeReplace"]')).not.toBeNull());
    await act(async () => button("restoreModeReplace").click());
    await act(async () => container.querySelector("form")?.requestSubmit());
    await vi.waitFor(() => expect(container.textContent).toContain("restoreFailureChanged: app"));
    await act(async () => change(
      container.querySelector<HTMLInputElement>('input[placeholder="restoreConfirmPlaceholder"]'),
      "REPLACE backup-1 ON profile-1 AT /srv/app STATE abc123",
    ));
    expect(button("restoreExecute").disabled).toBe(true);
    expect(commands.executeSourceReplacement).not.toHaveBeenCalled();
  });

  it("suggests a new destination and preserves it across mode changes", async () => {
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    const target = await vi.waitFor(() => requiredInput('input[placeholder="deployTargetPathHint"]'));
    expect(target.value).toBe("/srv/app-restored");

    await act(async () => button("restoreModeReplace").click());
    expect(target.value).toBe("/srv/app");
    expect(target.readOnly).toBe(true);

    await act(async () => button("restoreModeSeparate").click());
    expect(target.value).toBe("/srv/app-restored");
    expect(target.readOnly).toBe(false);
  });

  it("drops a previewed plan when another backup is chosen", async () => {
    commands.listBackups.mockResolvedValue([
      { backupId: "backup-1", sealedAt: "2026-07-17T00:00:00Z", verification: "verified" },
      { backupId: "backup-2", sealedAt: "2026-07-18T00:00:00Z", verification: "verified" },
    ]);
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    await vi.waitFor(() => expect(container.querySelector('[data-backup-id="backup-2"]')).not.toBeNull());
    await act(async () => container.querySelector("form")?.requestSubmit());
    await vi.waitFor(() => expect(button("restoreExecute")).toBeDefined());
    await act(async () => container.querySelector<HTMLButtonElement>('[data-backup-id="backup-2"]')?.click());
    await vi.waitFor(() => expect(container.textContent).not.toContain("restoreExecute"));
  });

  it("replaces original data only on the server the backup came from", async () => {
    commands.listSshProfiles.mockResolvedValue([
      { profileId: "profile-1", label: "Source", host: "vds.example", port: 22, user: "root", authKind: "ssh_key" },
      { profileId: "profile-2", label: "Other", host: "other.example", port: 22, user: "root", authKind: "ssh_key" },
    ]);
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    const server = await vi.waitFor(() => {
      const select = container.querySelector<HTMLSelectElement>('select[aria-label="deployTargetProfile"]');
      if (!select) throw new Error("missing server select");
      return select;
    });
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")?.set?.call(server, "profile-2");
      server.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () => button("restoreModeReplace").click());
    await act(async () => container.querySelector("form")?.requestSubmit());
    await vi.waitFor(() => expect(commands.previewSourceReplacement).toHaveBeenCalled());
    expect(commands.previewSourceReplacement).toHaveBeenCalledWith(expect.objectContaining({ targetProfileId: "profile-1" }));
  });

  it("locks the backup list while a restore runs so its Cancel stays reachable", async () => {
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    await vi.waitFor(() => expect(container.querySelector('[data-backup-id="backup-1"]')).not.toBeNull());
    await act(async () => change(container.querySelector<HTMLInputElement>('input[placeholder="deployTargetPathHint"]'), "/srv/restored"));
    await act(async () => container.querySelector("form")?.requestSubmit());
    await vi.waitFor(() => expect(button("restoreExecute")).toBeDefined());
    await act(async () => change(container.querySelector<HTMLInputElement>('input[placeholder="restoreConfirmPlaceholder"]'), "DEPLOY backup-1 TO profile-1 AT /srv/restored"));
    await act(async () => button("restoreExecute").click());
    await vi.waitFor(() => expect(button("restoreCancelRunning")).toBeDefined());
    expect(container.querySelector<HTMLButtonElement>('[data-backup-id="backup-1"]')?.disabled).toBe(true);
  });

  it("spells out the server, stopped services, safety backup and rollback copy before replacing", async () => {
    commands.previewSourceReplacement.mockResolvedValueOnce({
      backupId: "backup-1", targetProfileId: "profile-1", root: "/srv/app", containers: ["app", "db"], replaces: ["/srv/app"],
      conflicts: ["container_image_changed:app:1.2.3"], safetyBackupRequired: true, serviceStopRequired: true,
      confirmation: "REPLACE backup-1", rollbackPath: "/srv/.guardian-rollback/app",
    });
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    await vi.waitFor(() => expect(container.querySelector('[aria-label="restoreModeReplace"]')).not.toBeNull());
    await act(async () => button("restoreModeReplace").click());
    await act(async () => container.querySelector("form")?.requestSubmit());
    await vi.waitFor(() => expect(container.querySelector(".confirm__facts")).not.toBeNull());
    const facts = Object.fromEntries([...container.querySelectorAll(".confirm__fact")].map((fact) => [fact.querySelector("dt")?.textContent, fact.querySelector("dd")?.textContent]));
    expect(facts).toMatchObject({
      deployTargetProfile: "Source", restorePlanServiceStop: "app, db",
      restorePlanSafetyBackup: "restorePlanSafetyBackupAuto", restorePlanRollbackPath: "/srv/.guardian-rollback/app",
    });
    expect(container.textContent).toContain("restoreFailureChanged: app:1.2.3");
  });

  it("explains why replacing is unavailable instead of silently disabling it", async () => {
    commands.inspectRestoreBackup.mockResolvedValue({
      backupId: "backup-1", sourceProfileId: "gone", roots: ["/srv/app"], dockerWorkloads: [], entries: [], totalEntries: 0, replacementAvailable: true,
    });
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    const replace = await vi.waitFor(() => {
      const radio = container.querySelector<HTMLButtonElement>('[role="radio"][aria-disabled="true"]');
      if (!radio) throw new Error("replace radio not yet disabled");
      return radio;
    });
    expect(replace.getAttribute("data-tip")).toContain("restoreReplaceUnavailable");
    await act(async () => replace.click());
    expect(replace.getAttribute("aria-checked")).toBe("false");
  });

  it("moves the backup choice with the arrow keys", async () => {
    commands.listBackups.mockResolvedValue([
      { backupId: "backup-1", sealedAt: "2026-07-17T00:00:00Z", verification: "verified" },
      { backupId: "backup-2", sealedAt: "2026-07-18T00:00:00Z", verification: "verified" },
    ]);
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    await vi.waitFor(() => expect(container.querySelector('[data-backup-id="backup-2"]')).not.toBeNull());
    const list = container.querySelector<HTMLElement>('[role="listbox"]');
    const selected = () => container.querySelector('[role="option"][aria-selected="true"]')?.getAttribute("data-backup-id");
    await act(async () => list?.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true })));
    expect(selected()).toBe("backup-2");
    await act(async () => list?.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true })));
    expect(selected()).toBe("backup-1");
    expect(container.querySelectorAll('[role="option"][tabindex="0"]')).toHaveLength(1);
  });

  function button(label: string): HTMLButtonElement {
    const match = [...container.querySelectorAll("button")]
      .find((candidate) => candidate.textContent?.includes(label) || candidate.getAttribute("aria-label") === label);
    if (!match) throw new Error(`missing button: ${label}`);
    return match;
  }
});

function requiredInput(selector: string): HTMLInputElement {
  const input = document.querySelector<HTMLInputElement>(selector);
  if (!input) throw new Error(`missing input: ${selector}`);
  return input;
}

function change(input: HTMLInputElement | null, value: string): void {
  if (!input) throw new Error("missing input");
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}
