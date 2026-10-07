import { useEffect, useState } from "react";
import type { Translate } from "../../i18n";
import { listRepositories, listSshProfiles, type RepositorySummary, type SshProfileSummary } from "../../shared/commands";
import { safeErrorText } from "../../shared/safe-error";

function retainedId<T>(current: string, items: T[], read: (item: T) => string): string {
  return items.some((item) => read(item) === current) ? current : items[0] ? read(items[0]) : "";
}

/** Servers and recovery-ready repositories, with the currently chosen pair kept stable across reloads. */
export function useCaptureResources(resourcesRevision: number, t: Translate) {
  const [profiles, setProfiles] = useState<SshProfileSummary[]>([]);
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [profileId, setProfileId] = useState("");
  const [repositoryId, setRepositoryId] = useState("");
  const [loading, setLoading] = useState(true);
  const [failure, setFailure] = useState<string>();
  const [retryRevision, setRetryRevision] = useState(0);
  useEffect(() => {
    let active = true;
    setLoading(true); setFailure(undefined);
    Promise.all([listSshProfiles(), listRepositories()])
      .then(([nextProfiles, nextRepositories]) => {
        if (!active) return;
        const ready = nextRepositories.filter((repository) => repository.recoveryReady);
        setProfiles(nextProfiles); setRepositories(ready);
        setProfileId((current) => retainedId(current, nextProfiles, (item) => item.profileId));
        setRepositoryId((current) => retainedId(current, ready, (item) => item.repositoryId));
      })
      .catch((error: unknown) => { if (active) setFailure(safeErrorText(error, t("readinessErrorFallback"))); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [resourcesRevision, retryRevision, t]);
  return {
    profiles, repositories, profileId, repositoryId, loading, failure,
    ready: profiles.length > 0 && repositories.length > 0,
    setProfileId, setRepositoryId, retry: () => setRetryRevision((current) => current + 1),
  };
}
