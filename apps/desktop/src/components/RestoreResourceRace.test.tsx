import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackupRestoreDescription, BackupSummary } from "../shared/commands";
import { RestorePanel } from "./RestorePanel";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({
  inspectRestoreBackup: vi.fn(), listBackups: vi.fn(), listRepositories: vi.fn(), listSshProfiles: vi.fn(),
}));

vi.mock("../shared/commands", async (importOriginal) => ({
  ...await importOriginal<typeof import("../shared/commands")>(), ...commands, hasTauriRuntime: () => true,
}));

describe("restore resource races", () => {
  let container: HTMLDivElement; let root: Root;
  beforeEach(() => {
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
    commands.listRepositories.mockResolvedValue([
      { repositoryId: "repo-1", label: "One", path: "D:/one", recoveryReady: true },
      { repositoryId: "repo-2", label: "Two", path: "D:/two", recoveryReady: true },
    ]);
    commands.listSshProfiles.mockResolvedValue([{ profileId: "server-1", label: "VDS", host: "vds.example", port: 22, user: "backup", authKind: "ssh_key" }]);
  });
  afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.clearAllMocks(); });

  it("ignores a late backup list from the previously selected repository", async () => {
    const first = deferred<BackupSummary[]>();
    commands.listBackups.mockImplementation((repositoryId: string) => repositoryId === "repo-1" ? first.promise : Promise.resolve([backup("backup-2")]));
    commands.inspectRestoreBackup.mockResolvedValue(description("backup-2", "/srv/two"));
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    const repository = await vi.waitFor(() => selectWithOption(container, "repo-2"));
    await act(async () => selectValue(repository, "repo-2"));
    await vi.waitFor(() => expect(container.querySelector('[data-backup-id="backup-2"]')).not.toBeNull());
    await act(async () => first.resolve([backup("backup-1")]));
    expect(container.querySelector('[data-backup-id="backup-1"]')).toBeNull();
  });

  it("ignores a late description from the previously selected backup", async () => {
    const first = deferred<BackupRestoreDescription>();
    commands.listRepositories.mockResolvedValue([{ repositoryId: "repo-1", label: "One", path: "D:/one", recoveryReady: true }]);
    commands.listBackups.mockResolvedValue([backup("backup-1"), backup("backup-2")]);
    commands.inspectRestoreBackup.mockImplementation((_repositoryId: string, backupId: string) => backupId === "backup-1" ? first.promise : Promise.resolve(description("backup-2", "/srv/two")));
    await act(async () => root.render(<RestorePanel t={(key) => key} />));
    const second = await vi.waitFor(() => {
      const row = container.querySelector<HTMLButtonElement>('[data-backup-id="backup-2"]');
      if (!row) throw new Error("missing backup row");
      return row;
    });
    await act(async () => second.click());
    await vi.waitFor(() => expect(container.textContent).toContain("/srv/two"));
    await act(async () => first.resolve(description("backup-1", "/srv/one")));
    expect(container.textContent).not.toContain("/srv/one");
  });

  it("retries resource loading without showing a false empty-storage state", async () => {
    commands.listRepositories.mockRejectedValueOnce(new Error("registry unavailable"));
    commands.listBackups.mockResolvedValue([backup("backup-1")]);
    commands.inspectRestoreBackup.mockResolvedValue(description("backup-1", "/srv/one"));
    await act(async () => root.render(<RestorePanel t={(key) => key} />));

    await vi.waitFor(() => expect(container.querySelector('[role="alert"]')).not.toBeNull());
    expect(container.textContent).not.toContain("restoreNoRepositories");
    await act(async () => retryButton(container).click());
    await vi.waitFor(() => expect(container.querySelector('[data-backup-id="backup-1"]')).not.toBeNull());
  });

  it("retries a failed backup list without claiming the storage is empty", async () => {
    commands.listBackups.mockRejectedValueOnce(new Error("backup index unavailable")).mockResolvedValueOnce([backup("backup-1")]);
    commands.inspectRestoreBackup.mockResolvedValue(description("backup-1", "/srv/one"));
    await act(async () => root.render(<RestorePanel t={(key) => key} />));

    await vi.waitFor(() => expect(container.querySelector('[role="alert"]')).not.toBeNull());
    expect(container.textContent).not.toContain("restoreNoBackups");
    await act(async () => retryButton(container).click());
    await vi.waitFor(() => expect(container.querySelector('[data-backup-id="backup-1"]')).not.toBeNull());
  });

  it("retries inspection of the selected verified backup", async () => {
    commands.listRepositories.mockResolvedValue([{ repositoryId: "repo-1", label: "One", path: "D:/one", recoveryReady: true }]);
    commands.listBackups.mockResolvedValue([backup("backup-1")]);
    commands.inspectRestoreBackup.mockRejectedValueOnce(new Error("manifest unavailable")).mockResolvedValueOnce(description("backup-1", "/srv/one"));
    await act(async () => root.render(<RestorePanel t={(key) => key} />));

    await vi.waitFor(() => expect(container.querySelector('[role="alert"]')).not.toBeNull());
    expect(container.textContent).not.toContain("/srv/one");
    await act(async () => retryButton(container).click());
    await vi.waitFor(() => expect(container.textContent).toContain("/srv/one"));
  });
});

function backup(backupId: string): BackupSummary { return { backupId, sealedAt: "2026-08-21T00:00:00Z", verification: "verified" }; }
function description(backupId: string, root: string): BackupRestoreDescription { return { backupId, sourceProfileId: "server-1", roots: [root], dockerWorkloads: [], entries: [], totalEntries: 0, replacementAvailable: true }; }
function selectWithOption(container: HTMLElement, value: string): HTMLSelectElement { const option = container.querySelector<HTMLOptionElement>(`option[value="${value}"]`); if (!(option?.parentElement instanceof HTMLSelectElement)) throw new Error(`missing option: ${value}`); return option.parentElement; }
function selectValue(select: HTMLSelectElement, value: string): void { const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")?.set; setter?.call(select, value); select.dispatchEvent(new Event("change", { bubbles: true })); }
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => { resolve = done; }); return { promise, resolve }; }
function retryButton(container: HTMLElement): HTMLButtonElement { const button = [...container.querySelectorAll("button")].find((item) => item.textContent?.includes("readinessRefresh")); if (!(button instanceof HTMLButtonElement)) throw new Error("Retry button not found"); return button; }
