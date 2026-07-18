import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CapturePlanPanel } from "./CapturePlanPanel";
import { SigningIdentityPanel } from "./SigningIdentityPanel";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({
  browseRemoteDirectory: vi.fn(),
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
      { profileId: "server-1", label: "VDS", host: "vds.example", port: 22, user: "backup" },
    ]);
    commands.listRepositories.mockResolvedValue([
      { repositoryId: "repo-1", label: "Archive", path: "D:/archive", recoveryReady: true },
    ]);
    commands.runCaptureSelection.mockResolvedValue({ backupId: "backup-1" });
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
      <CapturePlanPanel onPlansChanged={changed} resourcesRevision={0} t={(key) => key} />,
    ));
    await vi.waitFor(() => expect(button("browserOpen").disabled).toBe(false));
    await act(async () => button("browserOpen").click());
    const selection = await vi.waitFor(() => {
      const candidate = container.querySelector<HTMLInputElement>('input[aria-label="browserSelect srv"]');
      if (!candidate) throw new Error("Remote path selection was not rendered");
      return candidate;
    });
    await act(async () => selection.click());
    expect(button("backupReview").disabled).toBe(false);

    await act(async () => container.querySelector("form")?.requestSubmit());
    await vi.waitFor(() => expect(button("backupCreate")).toBeDefined());
    await act(async () => button("backupCreate").click());

    await vi.waitFor(() => expect(changed).toHaveBeenCalledOnce());
    expect(commands.runCaptureSelection).toHaveBeenCalledWith(expect.objectContaining({
      confirmation: "CREATE BACKUP FOR server-1 IN repo-1 abcdef123456",
    }));
  });

  function button(label: string): HTMLButtonElement {
    const match = [...container.querySelectorAll("button")]
      .find((candidate) => candidate.textContent?.includes(label));
    if (!(match instanceof HTMLButtonElement)) throw new Error(`Button not found: ${label}`);
    return match;
  }
});
