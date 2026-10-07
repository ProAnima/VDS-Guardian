import { useEffect, useState } from "react";
import type { Translate } from "../../i18n";
import { inspectRestoreBackup, type BackupRestoreDescription, type SshProfileSummary } from "../../shared/commands";
import { safeErrorText } from "../../shared/safe-error";
import { useLatest } from "../../shared/useLatest";

export type RestoreMode = "separate" | "replace";

function preferredProfileId(sourceProfileId: string, profiles: SshProfileSummary[]): string {
  return profiles.some((item) => item.profileId === sourceProfileId) ? sourceProfileId : profiles[0]?.profileId ?? "";
}

function suggestRestorePath(root: string | undefined): string {
  if (!root || root === "/") return "/restore";
  return `${root.replace(/\/$/u, "")}-restored`;
}

/** What the chosen backup contains, plus where and how the operator wants it restored. */
export function useRestoreSelection(profiles: SshProfileSummary[], repositoryId: string, backupId: string, t: Translate) {
  const [description, setDescription] = useState<BackupRestoreDescription>();
  const [profileId, setProfileId] = useState("");
  const [mode, setMode] = useState<RestoreMode>("separate");
  const [separatePath, setSeparatePath] = useState("");
  const [loading, setLoading] = useState(false);
  const [failure, setFailure] = useState<string>();
  const [retryRevision, setRetryRevision] = useState(0);
  const translate = useLatest(t);
  useEffect(() => {
    setDescription(undefined); setFailure(undefined);
    if (!repositoryId || !backupId) return;
    let active = true;
    setLoading(true);
    inspectRestoreBackup(repositoryId, backupId)
      .then((value) => {
        if (!active) return;
        setDescription(value);
        setProfileId(preferredProfileId(value.sourceProfileId, profiles));
        setSeparatePath(suggestRestorePath(value.roots[0]));
        if (!value.replacementAvailable || !profiles.some((item) => item.profileId === value.sourceProfileId)) setMode("separate");
      })
      .catch((error: unknown) => { if (active) setFailure(safeErrorText(error, translate.current("restoreErrorFallback"))); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [repositoryId, backupId, profiles, retryRevision, translate]);
  const replacementReady = Boolean(description?.replacementAvailable && profiles.some((item) => item.profileId === description.sourceProfileId));
  // Replacing the original data always targets the server the backup came from.
  const effectiveProfileId = mode === "replace" && description ? description.sourceProfileId : profileId;
  return {
    description, profileId: effectiveProfileId, setProfileId, mode, setMode, replacementReady, loading, failure,
    targetPath: mode === "replace" ? description?.roots[0] ?? "" : separatePath,
    setTargetPath: setSeparatePath, retry: () => setRetryRevision((current) => current + 1),
  };
}
