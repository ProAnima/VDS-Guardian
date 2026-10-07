import { useEffect, useState } from "react";
import type { Translate } from "../../i18n";
import { listRepositories, listSshProfiles, type RepositorySummary, type SshProfileSummary } from "../../shared/commands";
import { safeErrorText } from "../../shared/safe-error";
import { useLatest } from "../../shared/useLatest";

/** Repositories and servers that a restore can read from and write to. */
export function useRestoreResources(t: Translate, refreshKey = 0) {
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [profiles, setProfiles] = useState<SshProfileSummary[]>([]);
  const [repositoryId, setRepositoryId] = useState("");
  const [loading, setLoading] = useState(true);
  const [failure, setFailure] = useState<string>();
  const [retryRevision, setRetryRevision] = useState(0);
  const [loaded, setLoaded] = useState(false);
  const translate = useLatest(t);
  useEffect(() => {
    let active = true;
    setLoading(true); setFailure(undefined);
    Promise.all([listRepositories(), listSshProfiles()])
      .then(([nextRepositories, nextProfiles]) => {
        if (!active) return;
        setRepositories(nextRepositories); setProfiles(nextProfiles);
        setRepositoryId((current) => (nextRepositories.some((item) => item.repositoryId === current) ? current : nextRepositories[0]?.repositoryId ?? ""));
        setLoaded(true);
      })
      .catch((error: unknown) => { if (active) setFailure(safeErrorText(error, translate.current("restoreErrorFallback"))); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [retryRevision, refreshKey, translate]);
  return { repositories, profiles, repositoryId, setRepositoryId, loading: loading && !loaded, failure: loaded ? undefined : failure, retry: () => setRetryRevision((current) => current + 1) };
}
