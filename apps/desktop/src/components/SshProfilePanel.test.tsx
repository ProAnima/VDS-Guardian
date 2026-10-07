import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SshProfilePanel } from "./SshProfilePanel";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({
  deleteSshProfile: vi.fn(), enrollSshProfile: vi.fn(), listSshProfiles: vi.fn(), pickSshKeyPath: vi.fn(),
}));

vi.mock("../shared/commands", async (importOriginal) => ({
  ...await importOriginal<typeof import("../shared/commands")>(), ...commands, hasTauriRuntime: () => true,
}));

describe("SSH profile loading", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount()); container.remove(); vi.clearAllMocks();
  });

  it("does not flash an empty state or enrollment form while profiles load", async () => {
    let finish!: (profiles: never[]) => void;
    commands.listSshProfiles.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    await act(async () => root.render(<SshProfilePanel onProfilesChanged={vi.fn()} t={(key) => key} />));

    expect(container.textContent).toContain("readinessLoading");
    expect(container.textContent).not.toContain("serversEmpty");
    expect(container.textContent).not.toContain("setupServerTitle");

    await act(async () => finish([]));
    await vi.waitFor(() => expect(container.textContent).toContain("setupServerTitle"));
  });

  it("keeps enrollment collapsed when a saved server exists", async () => {
    commands.listSshProfiles.mockResolvedValue([
      { profileId: "server", label: "VDS", host: "vds.example", port: 22, user: "backup" },
    ]);
    await act(async () => root.render(<SshProfilePanel onProfilesChanged={vi.fn()} t={(key) => key} />));

    await vi.waitFor(() => expect(container.textContent).toContain("VDS"));
    expect(container.querySelector(".server-form")).toBeNull();
    expect(container.querySelector('[aria-label="serversAdd"]')).not.toBeNull();
  });

  it("does not show an empty state or form when the server registry cannot be read", async () => {
    commands.listSshProfiles.mockRejectedValueOnce(new Error("registry unavailable")).mockResolvedValueOnce([]);
    await act(async () => root.render(<SshProfilePanel onProfilesChanged={vi.fn()} t={(key) => key} />));

    await vi.waitFor(() => expect(container.querySelector('[role="alert"]')).not.toBeNull());
    expect(container.textContent).not.toContain("serversEmpty");
    expect(container.textContent).not.toContain("setupServerTitle");
    await act(async () => retryButton(container).click());
    await vi.waitFor(() => expect(container.textContent).toContain("setupServerTitle"));
    expect(commands.listSshProfiles).toHaveBeenCalledTimes(2);
  });
});

function retryButton(container: HTMLElement): HTMLButtonElement {
  const button = [...container.querySelectorAll("button")].find((item) => item.textContent?.includes("readinessRefresh"));
  if (!(button instanceof HTMLButtonElement)) throw new Error("Retry button not found");
  return button;
}
