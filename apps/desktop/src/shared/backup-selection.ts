import type { BackupSelectionItem } from "./commands";

export function togglePathSelection(items: BackupSelectionItem[], path: string): BackupSelectionItem[] {
  const exact = items.some((item) => item.kind === "remote_path" && item.absolutePath === path);
  if (exact) return removeExactPath(items, path);
  if (selectedPaths(items).some((selected) => pathIsInside(path, selected))) return items;
  return [...removeNestedPaths(items, path), { kind: "remote_path", absolutePath: path }];
}

function removeExactPath(items: BackupSelectionItem[], path: string): BackupSelectionItem[] {
  return items.filter((item) => item.kind !== "remote_path" || item.absolutePath !== path);
}

function removeNestedPaths(items: BackupSelectionItem[], parent: string): BackupSelectionItem[] {
  return items.filter((item) => item.kind !== "remote_path" || !pathIsInside(item.absolutePath, parent));
}

function selectedPaths(items: BackupSelectionItem[]): string[] {
  return items.flatMap((item) => {
    if (item.kind === "remote_path") return [item.absolutePath];
    if (item.kind === "docker_mount") return [item.capturablePath];
    return item.capturablePaths;
  });
}

function pathIsInside(path: string, parent: string): boolean {
  return parent === "/" ? path !== "/" : path.startsWith(`${parent}/`);
}
