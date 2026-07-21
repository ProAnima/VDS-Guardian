import { StrictMode } from "react";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteFileExplorer } from "./RemoteFileExplorer";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({ browseRemoteDirectory: vi.fn() }));
vi.mock("../shared/commands", async (importOriginal) => ({
  ...await importOriginal<typeof import("../shared/commands")>(), ...commands, hasTauriRuntime: () => true,
}));

describe("remote file explorer", () => {
  let container: HTMLDivElement; let root: Root;
  beforeEach(() => { container = document.createElement("div"); document.body.append(container); root = createRoot(container); });
  afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.clearAllMocks(); });

  it("navigates with breadcrumbs and exposes only safe selections", async () => {
    commands.browseRemoteDirectory.mockImplementation(async (_profile: string, path: string) => path === "/" ? rootPage : srvPage);
    const toggle = vi.fn();
    await act(async () => root.render(<RemoteFileExplorer profileId="server-1" selectedPaths={[]} onTogglePath={toggle} t={(key) => key} />));
    await vi.waitFor(() => expect(button("srv")).toBeDefined());
    await act(async () => button("srv").click());
    await vi.waitFor(() => expect(container.textContent).toContain("app.sqlite"));

    const symlink = input("browserSelect current");
    expect(symlink.disabled).toBe(true);
    expect(container.textContent).toContain("browserSymlinkReason");
    await act(async () => input("browserSelect app.sqlite").click());
    expect(toggle).toHaveBeenCalledWith("/srv/app.sqlite");
    await act(async () => button("/").click());
    expect(commands.browseRemoteDirectory).toHaveBeenLastCalledWith("server-1", "/");
  });

  it("keeps the current page visible when refresh fails and offers retry", async () => {
    commands.browseRemoteDirectory.mockResolvedValueOnce(rootPage).mockRejectedValueOnce(new Error("offline"));
    await act(async () => root.render(<RemoteFileExplorer profileId="server-1" selectedPaths={[]} onTogglePath={vi.fn()} t={(key) => key} />));
    await vi.waitFor(() => expect(button("srv")).toBeDefined());
    const refresh = container.querySelector<HTMLButtonElement>('button[aria-label="browserRefresh"]');
    await act(async () => refresh?.click());
    await vi.waitFor(() => expect(container.textContent).toContain("browserFailureTitle"));
    expect(button("srv")).toBeDefined(); expect(button("browserRetry")).toBeDefined();
  });

  it("marks descendants as included when their parent folder is selected", async () => {
    commands.browseRemoteDirectory.mockImplementation(async (_profile: string, path: string) => path === "/" ? rootPage : srvPage);
    await act(async () => root.render(<RemoteFileExplorer profileId="server-1" selectedPaths={["/srv"]} onTogglePath={vi.fn()} t={(key) => key} />));
    await vi.waitFor(() => expect(button("srv")).toBeDefined());
    await act(async () => button("srv").click());
    await vi.waitFor(() => expect(input("browserSelect app.sqlite").disabled).toBe(true));
    expect(input("browserSelect app.sqlite").checked).toBe(true);
    expect(container.textContent).toContain("browserCoveredReason /srv");
  });

  it("ignores a late directory result from the previously selected server", async () => {
    let resolveOld: ((page: typeof rootPage) => void) | undefined;
    commands.browseRemoteDirectory.mockImplementation((profile: string) => profile === "old"
      ? new Promise((resolve) => { resolveOld = resolve; })
      : Promise.resolve(newServerPage));
    await act(async () => root.render(<RemoteFileExplorer profileId="old" selectedPaths={[]} onTogglePath={vi.fn()} t={(key) => key} />));
    await act(async () => root.render(<RemoteFileExplorer profileId="new" selectedPaths={[]} onTogglePath={vi.fn()} t={(key) => key} />));
    await vi.waitFor(() => expect(button("home")).toBeDefined());
    await act(async () => resolveOld?.(rootPage));
    expect(container.textContent).toContain("home");
    expect(container.textContent).not.toContain("srv");
  });

  it("deduplicates the initial directory request in StrictMode", async () => {
    let resolveRequest: ((page: typeof rootPage) => void) | undefined;
    commands.browseRemoteDirectory.mockImplementation(() => new Promise((resolve) => { resolveRequest = resolve; }));
    await act(async () => root.render(<StrictMode><RemoteFileExplorer profileId="strict-server" selectedPaths={[]} onTogglePath={vi.fn()} t={(key) => key} /></StrictMode>));
    expect(commands.browseRemoteDirectory).toHaveBeenCalledTimes(1);
    await act(async () => resolveRequest?.(rootPage));
    await vi.waitFor(() => expect(button("srv")).toBeDefined());
  });

  function button(label: string): HTMLButtonElement {
    const match = [...container.querySelectorAll("button")].find((item) => item.textContent?.includes(label));
    if (!(match instanceof HTMLButtonElement)) throw new Error(`Button not found: ${label}`); return match;
  }
  function input(label: string): HTMLInputElement {
    const match = container.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`);
    if (!match) throw new Error(`Input not found: ${label}`); return match;
  }
});

const rootPage = { directory: "/", entries: [{ name: "srv", absolutePath: "/srv", kind: "directory", selectable: true }], truncated: false };
const newServerPage = { directory: "/", entries: [{ name: "home", absolutePath: "/home", kind: "directory", selectable: true }], truncated: false };
const srvPage = { directory: "/srv", entries: [
  { name: "app.sqlite", absolutePath: "/srv/app.sqlite", kind: "regular_file", size: 2048, modifiedAt: "2026-07-17T10:00:00Z", selectable: true },
  { name: "current", absolutePath: "/srv/current", kind: "symlink", selectable: false, unavailableReason: "symlink" },
], truncated: false };
