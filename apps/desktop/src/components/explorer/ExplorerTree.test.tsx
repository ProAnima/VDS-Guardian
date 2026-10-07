import { StrictMode, act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackupSelectionItem } from "../../shared/commands";
import { ExplorerTree } from "./ExplorerTree";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({ browseRemoteDirectory: vi.fn(), listDockerContainers: vi.fn() }));
vi.mock("../../shared/commands", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../shared/commands")>(), ...commands, hasTauriRuntime: () => true,
}));

const rootPage = { directory: "/", entries: [{ name: "srv", absolutePath: "/srv", kind: "directory", selectable: true }], truncated: false };
const srvPage = { directory: "/srv", entries: [
  { name: "app.sqlite", absolutePath: "/srv/app.sqlite", kind: "regular_file", size: 2048, modifiedAt: "2026-07-17T10:00:00Z", selectable: true },
  { name: "current", absolutePath: "/srv/current", kind: "symlink", selectable: false, unavailableReason: "symlink" },
], truncated: false };
const inventory = [
  { id: "abc", name: "shop-api", composeProject: "shop", state: "running", mounts: [
    { kind: "bind", destination: "/data", capturablePath: "/srv/shop/data" }, { kind: "tmpfs", destination: "/tmp" },
  ] },
  { id: "def", name: "cache", state: "exited", mounts: [{ kind: "volume", destination: "/c", capturablePath: "/v/cache" }] },
];

describe("explorer tree", () => {
  let container: HTMLDivElement; let root: Root;
  beforeEach(() => {
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
    commands.browseRemoteDirectory.mockImplementation(async (_profile: string, path: string) => (path === "/" ? rootPage : srvPage));
    commands.listDockerContainers.mockResolvedValue(inventory);
  });
  afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.clearAllMocks(); });

  const render = (props: Partial<Parameters<typeof ExplorerTree>[0]> = {}, strict = false) => {
    const tree = <ExplorerTree profileId="server-1" items={[]} onTogglePath={vi.fn()} onSetDocker={vi.fn()} t={(key) => key} {...props} />;
    return act(async () => root.render(strict ? <StrictMode>{tree}</StrictMode> : tree));
  };

  it("expands folders lazily and exposes only safe selections", async () => {
    const toggle = vi.fn();
    await render({ onTogglePath: toggle });
    await vi.waitFor(() => expect(row("srv")).toBeDefined());
    expect(commands.browseRemoteDirectory).toHaveBeenCalledTimes(1);
    await act(async () => chevron("srv").click());
    await vi.waitFor(() => expect(row("app.sqlite")).toBeDefined());

    expect(input("browserSelect current").disabled).toBe(true);
    expect(row("current").getAttribute("aria-description")).toBe("browserSymlinkReason");
    await act(async () => input("browserSelect app.sqlite").click());
    expect(toggle).toHaveBeenCalledWith("/srv/app.sqlite");
  });

  it("collapses a folder without refetching it on the next expand", async () => {
    await render();
    await vi.waitFor(() => expect(row("srv")).toBeDefined());
    await act(async () => chevron("srv").click());
    await vi.waitFor(() => expect(row("app.sqlite")).toBeDefined());
    await act(async () => chevron("srv").click());
    expect(container.textContent).not.toContain("app.sqlite");
    await act(async () => chevron("srv").click());
    expect(container.textContent).toContain("app.sqlite");
    expect(commands.browseRemoteDirectory).toHaveBeenCalledTimes(2);
  });

  it("marks descendants as included when their parent folder is selected", async () => {
    await render({ items: [{ kind: "remote_path", absolutePath: "/srv" }] });
    await vi.waitFor(() => expect(row("srv")).toBeDefined());
    await act(async () => chevron("srv").click());
    await vi.waitFor(() => expect(input("browserSelect app.sqlite").disabled).toBe(true));
    expect(input("browserSelect app.sqlite").checked).toBe(true);
    expect(row("app.sqlite").getAttribute("aria-description")).toBe("browserCoveredReason /srv");
  });

  it("keeps the tree and offers a retry when a folder fails to load", async () => {
    await render();
    await vi.waitFor(() => expect(row("srv")).toBeDefined());
    commands.browseRemoteDirectory.mockRejectedValueOnce(new Error("offline"));
    await act(async () => chevron("srv").click());
    await vi.waitFor(() => expect(container.querySelector('[data-note="failure"]')).not.toBeNull());
    expect(row("srv")).toBeDefined();
    const retry = container.querySelector<HTMLButtonElement>('[data-note="failure"] button');
    await act(async () => retry?.click());
    await vi.waitFor(() => expect(row("app.sqlite")).toBeDefined());
  });

  it("ignores a late directory result from the previously selected server", async () => {
    let resolveOld: ((page: typeof rootPage) => void) | undefined;
    commands.browseRemoteDirectory.mockImplementation((profile: string) => profile === "old"
      ? new Promise((resolve) => { resolveOld = resolve; })
      : Promise.resolve({ ...rootPage, entries: [{ name: "home", absolutePath: "/home", kind: "directory", selectable: true }] }));
    await render({ profileId: "old" });
    await render({ profileId: "new" });
    await vi.waitFor(() => expect(row("home")).toBeDefined());
    await act(async () => resolveOld?.(rootPage));
    expect(container.textContent).toContain("home");
    expect(container.textContent).not.toContain("srv");
  });

  it("deduplicates the initial directory request in StrictMode", async () => {
    let resolveRequest: ((page: typeof rootPage) => void) | undefined;
    commands.browseRemoteDirectory.mockImplementation(() => new Promise((resolve) => { resolveRequest = resolve; }));
    await render({ profileId: "strict-server" }, true);
    expect(commands.browseRemoteDirectory).toHaveBeenCalledTimes(1);
    await act(async () => resolveRequest?.(rootPage));
    await vi.waitFor(() => expect(row("srv")).toBeDefined());
  });

  it("shows Docker projects and containers next to the files as soon as a server is chosen", async () => {
    await render();
    await vi.waitFor(() => expect(row("shop")).toBeDefined());
    expect(row("cache")).toBeDefined();
    expect(container.querySelector('[data-state="active"]')).not.toBeNull();
    expect(container.querySelector('[data-state="stopped"]')).not.toBeNull();
  });

  it("selects a compose project as one group and a container as its persistent mounts", async () => {
    const setDocker = vi.fn();
    await render({ onSetDocker: setDocker });
    await vi.waitFor(() => expect(row("shop")).toBeDefined());
    await act(async () => input("browserSelect shop").click());
    expect(setDocker).toHaveBeenLastCalledWith([{ kind: "docker_group", groupId: "shop", capturablePaths: ["/srv/shop/data"] }], true);
    await act(async () => input("browserSelect cache").click());
    expect(setDocker).toHaveBeenLastCalledWith([{ kind: "docker_mount", containerId: "def", mountDestination: "/c", capturablePath: "/v/cache" }], true);
  });

  it("explains a non-persistent mount instead of offering it", async () => {
    await render();
    await vi.waitFor(() => expect(row("shop")).toBeDefined());
    await act(async () => chevron("shop").click());
    await act(async () => chevron("shop-api").click());
    await vi.waitFor(() => expect(row("/tmp")).toBeDefined());
    expect(input("browserSelect /tmp").disabled).toBe(true);
    expect(row("/tmp").getAttribute("aria-description")).toBe("dockerNotPersistent");
  });

  it("shows mounts covered by a selected group as included and unchecks nothing silently", async () => {
    const items: BackupSelectionItem[] = [{ kind: "docker_group", groupId: "shop", capturablePaths: ["/srv/shop/data"] }];
    await render({ items });
    await vi.waitFor(() => expect(row("shop")).toBeDefined());
    expect(input("browserSelect shop").checked).toBe(true);
    await act(async () => chevron("shop").click());
    await act(async () => chevron("shop-api").click());
    await vi.waitFor(() => expect(row("/data")).toBeDefined());
    expect(input("browserSelect /data").checked).toBe(true);
    expect(input("browserSelect /data").disabled).toBe(true);
  });

  it("keeps the file tree usable when the Docker inventory is unavailable", async () => {
    commands.listDockerContainers.mockRejectedValue(new Error("no docker"));
    await render();
    await vi.waitFor(() => expect(row("srv")).toBeDefined());
    expect(container.querySelector(".tree-row__alert")).not.toBeNull();
  });

  it("puts exactly one row in the Tab order and keeps it on the last focused row", async () => {
    await render();
    await vi.waitFor(() => expect(row("srv")).toBeDefined());
    const stops = () => [...container.querySelectorAll<HTMLElement>("[data-row]")].filter((item) => item.tabIndex === 0);
    expect(stops()).toHaveLength(1);
    await act(async () => row("srv").focus());
    await act(async () => chevron("srv").click());
    await vi.waitFor(() => expect(row("app.sqlite")).toBeDefined());
    expect(stops()).toEqual([row("srv")]);
  });

  it("moves with the arrow keys and selects with Space", async () => {
    const toggle = vi.fn();
    await render({ onTogglePath: toggle });
    await vi.waitFor(() => expect(row("srv")).toBeDefined());
    await act(async () => row("srv").focus());
    await act(async () => row("srv").dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true })));
    expect(toggle).toHaveBeenCalledWith("/srv");
    await act(async () => row("srv").dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true })));
    expect(document.activeElement).not.toBe(row("srv"));
  });

  it("reloads a section with R", async () => {
    await render();
    await vi.waitFor(() => expect(row("srv")).toBeDefined());
    const calls = commands.browseRemoteDirectory.mock.calls.length;
    const files = [...container.querySelectorAll<HTMLElement>("[data-row]")].find((item) => item.dataset.tone === "section" && item.textContent?.includes("explorerFiles"));
    await act(async () => files?.dispatchEvent(new KeyboardEvent("keydown", { key: "r", bubbles: true })));
    await vi.waitFor(() => expect(commands.browseRemoteDirectory.mock.calls.length).toBe(calls + 1));
  });

  function row(label: string): HTMLElement {
    const match = [...container.querySelectorAll<HTMLElement>("[data-row]")].find((item) => item.querySelector(".tree-row__name")?.firstChild?.textContent === label);
    if (!match) throw new Error(`Row not found: ${label}`);
    return match;
  }
  function chevron(label: string): HTMLButtonElement {
    const match = row(label).querySelector<HTMLButtonElement>(".tree-row__chevron");
    if (!match) throw new Error(`Chevron not found: ${label}`);
    return match;
  }
  function input(label: string): HTMLInputElement {
    const match = container.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`);
    if (!match) throw new Error(`Input not found: ${label}`);
    return match;
  }
});
