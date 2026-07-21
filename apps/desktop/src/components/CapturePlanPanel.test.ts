import { describe, expect, it } from "vitest";
import type { BackupSelectionItem } from "../shared/commands";
import { togglePathSelection } from "../shared/backup-selection";

describe("backup path selection", () => {
  it("does not add a path already covered by a selected parent", () => {
    const items: BackupSelectionItem[] = [{ kind: "remote_path", absolutePath: "/srv" }];
    expect(togglePathSelection(items, "/srv/app/config")).toBe(items);
  });

  it("replaces nested filesystem paths with their selected parent", () => {
    const mount: BackupSelectionItem = {
      kind: "docker_mount", containerId: "app", mountDestination: "/data", capturablePath: "/srv/app/data",
    };
    const items: BackupSelectionItem[] = [
      { kind: "remote_path", absolutePath: "/srv/app/config" }, mount,
    ];
    expect(togglePathSelection(items, "/srv")).toEqual([
      mount, { kind: "remote_path", absolutePath: "/srv" },
    ]);
  });

  it("removes an exact selected path", () => {
    const items: BackupSelectionItem[] = [{ kind: "remote_path", absolutePath: "/" }];
    expect(togglePathSelection(items, "/")).toEqual([]);
  });
});
