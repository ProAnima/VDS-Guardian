import { ShieldCheck } from "lucide-react";
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

export function BackupList({ backups, selectedId, onSelect, disabled, t }: BackupListProps) {
  return (
    <div className="backup-list" role="listbox" aria-label={t("restoreBackupsTitle")}>
      {backups.map((backup) => (
        <button
          className="backup-list__row" type="button" role="option" key={backup.backupId} data-backup-id={backup.backupId}
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

function shortId(backupId: string): string {
  return backupId.length > 14 ? `…${backupId.slice(-8)}` : backupId;
}
