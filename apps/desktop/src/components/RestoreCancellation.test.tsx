import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RestorePanel } from "./RestorePanel";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({
  cancelJob: vi.fn(),
  executeRestore: vi.fn(),
  listBackups: vi.fn(),
  listRepositories: vi.fn(),
  previewRestore: vi.fn(),
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
    commands.previewRestore.mockResolvedValue({
      backupId: "backup-1",
      destination: "D:/restore",
      confirmation: "RESTORE backup-1 TO D:/restore",
      payload: "payload/filesystem.tar.zst.enc",
    });
    commands.executeRestore.mockReturnValue(new Promise(() => undefined));
    commands.cancelJob.mockResolvedValue(true);
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
      container.querySelector<HTMLInputElement>('input[placeholder="restoreDestinationHint"]'),
      "D:/restore",
    ));
    await act(async () => container.querySelector("form")?.requestSubmit());
    await vi.waitFor(() => expect(button("restoreExecute")).toBeDefined());
    await act(async () => change(
      container.querySelector<HTMLInputElement>('input[placeholder="restoreConfirmPlaceholder"]'),
      "RESTORE backup-1 TO D:/restore",
    ));
    await act(async () => button("restoreExecute").click());
    await vi.waitFor(() => expect(button("restoreCancelRunning")).toBeDefined());
    const request = commands.executeRestore.mock.calls[0]?.[0] as { runId?: string };
    expect(request.runId).toMatch(/^[0-9a-f-]{36}$/);
    await act(async () => button("restoreCancelRunning").click());
    expect(commands.cancelJob).toHaveBeenCalledWith(request.runId);
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
