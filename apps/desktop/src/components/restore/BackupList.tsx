import { ShieldCheck } from "lucide-react";
import type { KeyboardEvent } from "react";
import type { Translate } from "../../i18n";
import type { BackupSummary } from "../../shared/commands";
import { formatDateTime } from "../explorer/format";

interface BackupListProps {
  backups: BackupSummary[];
  selectedId: string;
  onSelect: (backupId: string) => void;
  /** While a restore runs the choice is locked, so its plan and Cancel stay on screen. */
  disabled?: boolean;
  t: Translate;
}

/** A single-select listbox: one tab stop on the selected backup, arrows/Home/End move the choice. */
export function BackupList({ backups, selectedId, onSelect, disabled, t }: BackupListProps) {
  const selectedIndex = Math.max(0, backups.findIndex((backup) => backup.backupId === selectedId));
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const next = nextIndex(event.key, selectedIndex, backups.length);
    const backup = next === undefined ? undefined : backups[next];
    if (next === undefined || !backup || disabled) return;
    event.preventDefault();
    onSelect(backup.backupId);
    event.currentTarget.querySelectorAll<HTMLElement>("[role='option']")[next]?.focus();
  };
  return (
    <div className="backup-list" role="listbox" aria-label={t("restoreBackupsTitle")} onKeyDown={onKeyDown}>
      {backups.map((backup, index) => (
        <button
          className="backup-list__row" type="button" role="option" key={backup.backupId} data-backup-id={backup.backupId}
          tabIndex={index === selectedIndex ? 0 : -1}
          aria-selected={backup.backupId === selectedId} aria-disabled={disabled || undefined} disabled={disabled} data-tip={backup.backupId} data-tip-side="start"
          onClick={() => onSelect(backup.backupId)}
        >
          <ShieldCheck size={15} aria-hidden="true" />
          <span className="backup-list__date">{formatDateTime(backup.sealedAt) || backup.sealedAt}</span>
          <code>{shortId(backup.backupId)}</code>
        </button>
      ))}
    </div>
  );
}

function nextIndex(key: string, current: number, count: number): number | undefined {
  if (count === 0) return undefined;
  if (key === "ArrowDown") return Math.min(count - 1, current + 1);
  if (key === "ArrowUp") return Math.max(0, current - 1);
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  return undefined;
}

function shortId(backupId: string): string {
  return backupId.length > 14 ? `…${backupId.slice(-8)}` : backupId;
}
