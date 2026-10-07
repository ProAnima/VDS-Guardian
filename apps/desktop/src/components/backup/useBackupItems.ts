import { useState } from "react";
import { togglePathSelection } from "../../shared/backup-selection";
import type { BackupSelectionItem } from "../../shared/commands";
import { itemKey, setItems } from "../explorer/selection";

/** The operator's explicit selection. */
export function useBackupItems() {
  const [items, update] = useState<BackupSelectionItem[]>([]);
  return {
    items,
    togglePath: (path: string) => update((current) => togglePathSelection(current, path)),
    setDocker: (changed: BackupSelectionItem[], selected: boolean) => update((current) => setItems(current, changed, selected)),
    remove: (item: BackupSelectionItem) => update((current) => current.filter((candidate) => itemKey(candidate) !== itemKey(item))),
    clear: () => update(() => []),
  };
}

export type BackupItems = ReturnType<typeof useBackupItems>;
