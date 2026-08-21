import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FoundationStatus } from "../shared/commands";
import { Dashboard } from "./Dashboard";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const status: FoundationStatus = {
  product: "VDS Guardian",
  version: "0.1.0",
  iteration: "Release 0.1 validation",
  liveOperationsEnabled: true,
};

describe("Dashboard", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
  });

  it("shows one compact workflow and opens each operator action", async () => {
    const addServer = vi.fn();
    const runBackup = vi.fn();
    const restore = vi.fn();
    await act(async () => root.render(
      <Dashboard status={status} t={(key) => key} onAddServer={addServer} onRunBackup={runBackup} onRestore={restore} />,
    ));

    expect(container.textContent).toContain("securityBody");
    expect(container.textContent).not.toContain("lockedBody");
    expect(container.textContent).toContain("dashboardStartTitle");
    expect(container.querySelectorAll(".workflow-step")).toHaveLength(3);
    expect([...container.querySelectorAll("button")].filter((button) => button.textContent?.includes("runBackup"))).toHaveLength(1);
    await act(async () => container.querySelector<HTMLButtonElement>(".workflow-step")?.click());
    expect(addServer).toHaveBeenCalledOnce();
  });

  it("keeps the fail-closed explanation when live operations are disabled", async () => {
    await act(async () => root.render(
      <Dashboard status={{ ...status, liveOperationsEnabled: false }} t={(key) => key} onAddServer={vi.fn()} onRunBackup={vi.fn()} onRestore={vi.fn()} />,
    ));

    expect(container.textContent).toContain("lockedTitle");
    expect(container.textContent).toContain("lockedBody");
  });
});
