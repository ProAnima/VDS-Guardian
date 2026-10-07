import type { BackupSelectionItem } from "../../shared/commands";

export function itemKey(item: BackupSelectionItem): string { return JSON.stringify(item); }

export function pathsForItem(item: BackupSelectionItem): string[] {
  if (item.kind === "remote_path") return [item.absolutePath];
  if (item.kind === "docker_mount") return [item.capturablePath];
  return item.capturablePaths;
}

export function remotePaths(items: BackupSelectionItem[]): string[] {
  return items.flatMap((item) => (item.kind === "remote_path" ? [item.absolutePath] : []));
}

/** A selected ancestor already includes `path`; the tree shows it as covered rather than selectable again. */
export function coveringAncestor(paths: string[], path: string): string | undefined {
  return paths.find((selected) => selected !== path && (selected === "/" || path.startsWith(`${selected}/`)));
}

export function hasItem(items: BackupSelectionItem[], item: BackupSelectionItem): boolean {
  const key = itemKey(item);
  return items.some((candidate) => itemKey(candidate) === key);
}

/** Add every missing item, or — when `selected` is false — remove all of them in one step. */
export function setItems(items: BackupSelectionItem[], changed: BackupSelectionItem[], selected: boolean): BackupSelectionItem[] {
  if (!selected) {
    const removed = new Set(changed.map(itemKey));
    return items.filter((item) => !removed.has(itemKey(item)));
  }
  return [...items, ...changed.filter((item) => !hasItem(items, item))];
}

/** How a Docker mount path is already part of the selection, if at all. */
export function pathCoverage(items: BackupSelectionItem[], path: string): "selected" | "covered" | undefined {
  const mount = items.some((item) => item.kind === "docker_mount" && item.capturablePath === path);
  if (mount) return "selected";
  const group = items.some((item) => item.kind === "docker_group" && item.capturablePaths.includes(path));
  if (group || remotePaths(items).includes(path) || coveringAncestor(remotePaths(items), path)) return "covered";
  return undefined;
}
