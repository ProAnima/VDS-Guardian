import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FoundationStatus } from "../shared/commands";
import { Dashboard } from "./Dashboard";

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

const status: FoundationStatus = {
  product: "VDS Guardian",
  version: "0.1.0",
  iteration: "Release 0.1 validation",
  liveOperationsEnabled: true,
};

const profile = { profileId: "p", label: "VDS", host: "h", port: 22, user: "backup" };
const repository = { repositoryId: "r", label: "Disk", path: "D:\\b", recoveryReady: true };

describe("Dashboard", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    commands.getSigningIdentityStatus.mockResolvedValue({ state: "ready", identity: null });
    commands.listRepositories.mockResolvedValue([repository]);
    commands.listSshProfiles.mockResolvedValue([profile]);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });

  const render = (override: Partial<FoundationStatus> = {}, handlers = { addServer: vi.fn(), runBackup: vi.fn(), restore: vi.fn() }) =>
    act(async () => root.render(
      <Dashboard status={{ ...status, ...override }} t={(key) => key} onAddServer={handlers.addServer} onRunBackup={handlers.runBackup} onRestore={handlers.restore} />,
    ));

  it("shows one compact workflow and opens each operator action", async () => {
    const handlers = { addServer: vi.fn(), runBackup: vi.fn(), restore: vi.fn() };
    await render({}, handlers);

    expect(container.textContent).toContain("dashboardStartTitle");
    expect(container.querySelectorAll(".workflow-step")).toHaveLength(3);
    expect([...container.querySelectorAll("button")].filter((button) => button.textContent?.includes("runBackup"))).toHaveLength(1);
    await act(async () => container.querySelector<HTMLButtonElement>(".workflow-step")?.click());
    expect(handlers.addServer).toHaveBeenCalledOnce();
  });

  it("reports ready only when identity, storage, and a server are all set up", async () => {
    await render();

    expect(container.querySelector(".overview-status")?.getAttribute("data-state")).toBe("ready");
    expect(container.textContent).toContain("securityBody");
  });

  it("does not claim readiness on a fresh install and names what is still open", async () => {
    commands.getSigningIdentityStatus.mockResolvedValue({ state: "not_enrolled", identity: null });
    commands.listRepositories.mockResolvedValue([]);
    commands.listSshProfiles.mockResolvedValue([]);
    await render();

    expect(container.querySelector(".overview-status")?.getAttribute("data-state")).toBe("attention");
    expect(container.querySelector(".overview-status")?.hasAttribute("data-ready")).toBe(false);
    expect(container.textContent).not.toContain("securityBody");
    expect(container.textContent).toContain("backupProtection");
    expect(container.textContent).toContain("backupServer");
  });

  it("does not claim readiness when a prerequisite check fails", async () => {
    commands.listSshProfiles.mockRejectedValue(new Error("boom"));
    await render();

    expect(container.querySelector(".overview-status")?.getAttribute("data-state")).toBe("attention");
    expect(container.textContent).toContain("readinessCheckFailed");
  });

  it("keeps the fail-closed explanation when live operations are disabled", async () => {
    await render({ liveOperationsEnabled: false });

    expect(container.textContent).toContain("lockedTitle");
    expect(container.textContent).toContain("lockedBody");
    expect(container.querySelector(".overview-status")?.getAttribute("data-state")).toBe("locked");
  });
});
