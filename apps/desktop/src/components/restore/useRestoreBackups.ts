import { useEffect, useState } from "react";
import type { Translate } from "../../i18n";
import { listBackups, type BackupSummary } from "../../shared/commands";
import { safeErrorText } from "../../shared/safe-error";
import { useLatest } from "../../shared/useLatest";

/** Verified backups of one repository; a late answer for a previously chosen repository is ignored. */
export function useRestoreBackups(repositoryId: string, t: Translate) {
  const [backups, setBackups] = useState<BackupSummary[]>([]);
  const [backupId, setBackupId] = useState("");
  const [loading, setLoading] = useState(false);
  const [failure, setFailure] = useState<string>();
  const [retryRevision, setRetryRevision] = useState(0);
  const translate = useLatest(t);
  useEffect(() => {
    setBackups([]); setBackupId(""); setFailure(undefined);
    if (!repositoryId) return;
    let active = true;
    setLoading(true);
    listBackups(repositoryId)
      .then((items) => { if (active) { setBackups(items); setBackupId(items[0]?.backupId ?? ""); } })
      .catch((error: unknown) => { if (active) setFailure(safeErrorText(error, translate.current("restoreErrorFallback"))); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [repositoryId, retryRevision, translate]);
  return { backups, backupId, setBackupId, loading, failure, retry: () => setRetryRevision((current) => current + 1) };
}
