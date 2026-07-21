import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RepositoryPanel } from "./RepositoryPanel";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({
  deleteRepository: vi.fn(), initializeRepositoryRecovery: vi.fn(), listRepositories: vi.fn(),
  pickRepositoryPath: vi.fn(), registerRepository: vi.fn(), updateRepositoryPath: vi.fn(),
}));

vi.mock("../shared/commands", async (importOriginal) => ({
  ...await importOriginal<typeof import("../shared/commands")>(), ...commands, hasTauriRuntime: () => true,
}));

const repository = { repositoryId: "repository-1", label: "Archive", path: "D:/archive", recoveryReady: true };

describe("repository management", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
    commands.listRepositories.mockResolvedValue([repository]);
    commands.pickRepositoryPath.mockResolvedValue("E:/moved-archive");
    commands.updateRepositoryPath.mockResolvedValue({ ...repository, path: "E:/moved-archive" });
    commands.deleteRepository.mockResolvedValue(undefined);
  });

  afterEach(async () => {
    await act(async () => root.unmount()); container.remove(); vi.clearAllMocks();
  });

  it("changes the path only after selecting and saving an existing folder", async () => {
    await render();
    await act(async () => button("repositoryChangeFolder").click());
    await act(async () => button("setupBrowse", true).click());
    await act(async () => button("repositorySaveFolder").click());
    await vi.waitFor(() => expect(commands.updateRepositoryPath).toHaveBeenCalledWith({
      repositoryId: "repository-1", path: "E:/moved-archive",
    }));
  });

  it("requires an explicit second action before removing the registration", async () => {
    await render();
    await act(async () => button("repositoryDelete").click());
    expect(commands.deleteRepository).not.toHaveBeenCalled();
    expect(container.textContent).toContain("repositoryDeleteWarning");
    await act(async () => button("repositoryDelete", true).click());
    await vi.waitFor(() => expect(commands.deleteRepository).toHaveBeenCalledWith("repository-1"));
  });

  async function render() {
    await act(async () => root.render(<RepositoryPanel onRepositoriesChanged={vi.fn()} t={(key) => key} />));
    await vi.waitFor(() => expect(container.textContent).toContain("Archive"));
  }

  function button(label: string, last = false): HTMLButtonElement {
    const matches = [...container.querySelectorAll("button")].filter((item) => item.textContent?.includes(label));
    const match = last ? matches.at(-1) : matches[0];
    if (!(match instanceof HTMLButtonElement)) throw new Error(`Button not found: ${label}`);
    return match;
  }
});
