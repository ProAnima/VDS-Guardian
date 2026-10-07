import { useEffect, useState } from "react";
import { getSigningIdentityStatus, listRepositories, listSshProfiles } from "../shared/commands";
import { safeErrorText } from "../shared/safe-error";
import type { Translate } from "../i18n";
import type { SetupResources } from "./setup-readiness";

export interface LoadFailure { label: string; detail: string; }

interface SetupStatusLoad { resources?: SetupResources; failures: LoadFailure[]; }

/** One source of truth for "is setup complete", shared by every view that reports readiness. */
export function useSetupStatus(resourcesRevision: number, t: Translate) {
  const [resources, setResources] = useState<SetupResources>();
  const [failures, setFailures] = useState<LoadFailure[]>([]);
  const [loading, setLoading] = useState(true);
  const [reloadRevision, setReloadRevision] = useState(0);
  useEffect(() => {
    let active = true;
    void loadSetupStatus(t, (result) => { if (active) { setResources(result.resources); setFailures(result.failures); setLoading(false); } });
    return () => { active = false; };
  }, [resourcesRevision, reloadRevision, t]);
  return { resources, failures, loading, reload: () => { setLoading(true); setReloadRevision((value) => value + 1); } };
}

async function loadSetupStatus(t: Translate, update: (result: SetupStatusLoad) => void) {
  const [identity, repositories, profiles] = await Promise.allSettled([getSigningIdentityStatus(), listRepositories(), listSshProfiles()]);
  const failures = [
    loadFailure(t("readinessIdentity"), identity, t), loadFailure(t("readinessRepositoriesResource"), repositories, t),
    loadFailure(t("readinessServersResource"), profiles, t),
  ].flatMap((failure) => failure ? [failure] : []);
  update({ resources: {
    identity: identity.status === "fulfilled" ? identity.value : undefined,
    repositories: repositories.status === "fulfilled" ? repositories.value : undefined,
    profiles: profiles.status === "fulfilled" ? profiles.value : undefined,
  }, failures });
}

function loadFailure(label: string, result: PromiseSettledResult<unknown>, t: Translate): LoadFailure | undefined { return result.status === "rejected" ? { label, detail: errorText(result.reason, t) } : undefined; }
function errorText(error: unknown, t: Translate): string { return safeErrorText(error, t("readinessErrorFallback")); }
