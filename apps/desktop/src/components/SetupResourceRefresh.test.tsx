import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BackupWorkspace } from "./backup/BackupWorkspace";
import { SigningIdentityPanel } from "./SigningIdentityPanel";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({
  browseRemoteDirectory: vi.fn(),
  cancelJob: vi.fn(),
  listDockerContainers: vi.fn(),
  enrollSigningIdentity: vi.fn(),
  getSigningIdentityStatus: vi.fn(),
  listRepositories: vi.fn(),
  listSshProfiles: vi.fn(),
  previewCaptureSelection: vi.fn(),
  runCaptureSelection: vi.fn(),
}));

vi.mock("../shared/commands", async (importOriginal) => ({
  ...await importOriginal<typeof import("../shared/commands")>(),
  ...commands,
  hasTauriRuntime: () => true,
}));

describe("setup resource refresh", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    commands.getSigningIdentityStatus.mockResolvedValue({ state: "not_enrolled", identity: null });
    commands.enrollSigningIdentity.mockResolvedValue({
      disposition: "enrolled",
      identity: { credentialId: "signing-main", algorithm: "ed25519", keyId: "key-1" },
    });
    commands.listSshProfiles.mockResolvedValue([
      { profileId: "server-1", label: "VDS", host: "vds.example", port: 22, user: "backup", authKind: "ssh_key" },
    ]);
    commands.listRepositories.mockResolvedValue([
      { repositoryId: "repo-1", label: "Archive", path: "D:/archive", recoveryReady: true },
    ]);
    commands.listDockerContainers.mockResolvedValue([]);
    commands.runCaptureSelection.mockResolvedValue({ backupId: "backup-1" });
    commands.cancelJob.mockResolvedValue(true);
    commands.previewCaptureSelection.mockResolvedValue({
      profileId: "server-1", repositoryId: "repo-1", normalizedRoots: ["/srv"],
      logicalItems: [{ kind: "remote_path", absolutePath: "/srv" }], warnings: [],
      confirmation: "CREATE BACKUP FOR server-1 IN repo-1 abcdef123456",
    });
    commands.browseRemoteDirectory.mockResolvedValue({
      directory: "/",
      entries: [{ name: "srv", absolutePath: "/srv", kind: "directory", selectable: true }],
      truncated: false,
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });

  it("refreshes setup readiness after signing enrollment", async () => {
    const changed = vi.fn();
    await act(async () => root.render(<SigningIdentityPanel onIdentityChanged={changed} t={(key) => key} />));
    await vi.waitFor(() => expect(button("signingStart")).toBeDefined());

    await act(async () => button("signingStart").click());
    const acknowledgement = container.querySelector<HTMLInputElement>('input[type="checkbox"]');
    expect(acknowledgement).not.toBeNull();
    await act(async () => acknowledgement?.click());
    await act(async () => button("signingCreate").click());

    await vi.waitFor(() => expect(changed).toHaveBeenCalledOnce());
  });

  it("creates a backup directly from a reviewed selection", async () => {
    const changed = vi.fn();
    await act(async () => root.render(
      <Workspace onPlansChanged={changed} resourcesRevision={0} />,
    ));
    const selection = await vi.waitFor(() => {
      const candidate = container.querySelector<HTMLInputElement>('input[aria-label="browserSelect srv"]');
      if (!candidate) throw new Error("Remote path selection was not rendered");
      return candidate;
    });
    await act(async () => selection.click());
    expect(button("backupReview").disabled).toBe(false);

    await act(async () => button("backupReview").click());
    await vi.waitFor(() => expect(button("backupCreate")).toBeDefined());
    await act(async () => button("backupCreate").click());

    await vi.waitFor(() => expect(changed).toHaveBeenCalledOnce());
    expect(commands.runCaptureSelection).toHaveBeenCalledWith(expect.objectContaining({
      confirmation: "CREATE BACKUP FOR server-1 IN repo-1 abcdef123456",
    }));
    expect(container.textContent).toContain("captureSealed");
    expect([...container.querySelectorAll("button")].some((candidate) => candidate.textContent?.includes("backupCreate"))).toBe(false);
  });

  it("locks the editor and sends one cancellation request for a running backup", async () => {
    commands.runCaptureSelection.mockReturnValue(new Promise(() => undefined));
    await act(async () => root.render(
      <Workspace onPlansChanged={vi.fn()} resourcesRevision={0} />,
    ));
    const selection = await vi.waitFor(() => {
      const candidate = container.querySelector<HTMLInputElement>('input[aria-label="browserSelect srv"]');
      if (!candidate) throw new Error("Remote path selection was not rendered");
      return candidate;
    });
    await act(async () => selection.click());
    await act(async () => button("backupReview").click());
    await vi.waitFor(() => expect(button("backupCreate")).toBeDefined());
    await act(async () => button("backupCreate").click());
    const cancel = await vi.waitFor(() => button("captureCancel"));
    expect(container.querySelector(".backup-workspace__body")?.hasAttribute("inert")).toBe(true);
    await act(async () => cancel.click());
    expect(cancel.disabled).toBe(true);
    await act(async () => cancel.click());
    expect(commands.cancelJob).toHaveBeenCalledTimes(1);
  });

  it("keeps the file explorer hidden until backup prerequisites exist", async () => {
    commands.listRepositories.mockResolvedValue([]);
    commands.listSshProfiles.mockResolvedValue([]);
    await act(async () => root.render(
      <Workspace onPlansChanged={vi.fn()} resourcesRevision={0} />,
    ));

    await vi.waitFor(() => expect(container.textContent).toContain("backupSetupRequired"));
    expect(container.querySelector(".backup-workspace__body")).toBeNull();
  });

  it("offers a retry without showing setup-empty copy when backup resources fail", async () => {
    commands.listRepositories.mockRejectedValueOnce(new Error("registry unavailable"));
    await act(async () => root.render(
      <Workspace onPlansChanged={vi.fn()} resourcesRevision={0} />,
    ));

    await vi.waitFor(() => expect(container.querySelector('[role="alert"]')).not.toBeNull());
    expect(container.textContent).not.toContain("backupSetupRequired");
    expect(container.querySelector(".backup-workspace__body")).toBeNull();
    await act(async () => button("readinessRefresh").click());
    await vi.waitFor(() => expect(container.querySelector(".backup-workspace__body")).not.toBeNull());
    expect(commands.listRepositories).toHaveBeenCalledTimes(2);
  });

  it("preserves valid server and storage choices while resources refresh", async () => {
    commands.listSshProfiles.mockResolvedValue([
      { profileId: "server-1", label: "VDS 1", host: "one.example", port: 22, user: "backup", authKind: "ssh_key" },
      { profileId: "server-2", label: "VDS 2", host: "two.example", port: 22, user: "backup", authKind: "ssh_key" },
    ]);
    commands.listRepositories.mockResolvedValue([
      { repositoryId: "repo-1", label: "Archive 1", path: "D:/one", recoveryReady: true },
      { repositoryId: "repo-2", label: "Archive 2", path: "D:/two", recoveryReady: true },
    ]);
    const changed = vi.fn();
    await act(async () => root.render(<Workspace onPlansChanged={changed} resourcesRevision={0} />));
    const selects = await vi.waitFor(() => requiredSelects(container));
    await act(async () => selectValue(selects[0], "server-2"));
    await act(async () => selectValue(selects[1], "repo-2"));

    await act(async () => root.render(<Workspace onPlansChanged={changed} resourcesRevision={1} />));
    await vi.waitFor(() => expect(commands.listRepositories).toHaveBeenCalledTimes(2));
    expect(requiredSelects(container).map((select) => select.value)).toEqual(["server-2", "repo-2"]);
  });

  function button(label: string): HTMLButtonElement {
    const match = [...container.querySelectorAll("button")]
      .find((candidate) => candidate.textContent?.includes(label));
    if (!(match instanceof HTMLButtonElement)) throw new Error(`Button not found: ${label}`);
    return match;
  }
});

function Workspace({ onPlansChanged, resourcesRevision }: { onPlansChanged: () => void; resourcesRevision: number }) {
  return <BackupWorkspace onPlansChanged={onPlansChanged} resourcesRevision={resourcesRevision} notReady={<p>backupSetupRequired</p>} t={(key) => key} />;
}

function requiredSelects(container: HTMLElement): [HTMLSelectElement, HTMLSelectElement] {
  const selects = [...container.querySelectorAll("select")];
  if (selects.length !== 2) throw new Error("Backup selectors are not ready");
  return [selects[0]!, selects[1]!];
}

function selectValue(select: HTMLSelectElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")?.set;
  setter?.call(select, value);
  select.dispatchEvent(new Event("change", { bubbles: true }));
}
