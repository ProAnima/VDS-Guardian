import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DockerMountPicker } from "./DockerMountPicker";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const commands = vi.hoisted(() => ({ listDockerContainers: vi.fn() }));
vi.mock("../shared/commands", async (importOriginal) => ({
  ...await importOriginal<typeof import("../shared/commands")>(), ...commands, hasTauriRuntime: () => true,
}));

describe("docker mount picker", () => {
  let container: HTMLDivElement; let root: Root;
  beforeEach(() => { container = document.createElement("div"); document.body.append(container); root = createRoot(container); commands.listDockerContainers.mockResolvedValue(inventory); });
  afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.clearAllMocks(); });

  it("presents compose groups and persistent mounts as explicit selections", async () => {
    const toggle = vi.fn();
    await act(async () => root.render(<DockerMountPicker profileId="server-1" selectedItems={[]} onToggleItem={toggle} t={(key) => key} />));
    await act(async () => button("dockerShow").click());
    await vi.waitFor(() => expect(button("shop")).toBeDefined());
    expect(container.textContent).toContain("dockerActive");
    await act(async () => button("shop").click());
    expect(toggle).toHaveBeenCalledWith({ kind: "docker_group", groupId: "shop", capturablePaths: ["/srv/shop/data"] });
    await act(async () => button("/srv/shop/data").click());
    expect(toggle).toHaveBeenLastCalledWith({ kind: "docker_mount", containerId: "abc", mountDestination: "/data", capturablePath: "/srv/shop/data" });
  });

  function button(label: string): HTMLButtonElement {
    const match = [...container.querySelectorAll("button")].find((item) => item.textContent?.includes(label));
    if (!(match instanceof HTMLButtonElement)) throw new Error(`Button not found: ${label}`); return match;
  }
});

const inventory = [{
  id: "abc", name: "shop-api", composeProject: "shop", state: "running",
  mounts: [{ kind: "bind", destination: "/data", capturablePath: "/srv/shop/data" }, { kind: "tmpfs", destination: "/tmp" }],
}];
