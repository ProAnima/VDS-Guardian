import type { RepositorySummary, SigningIdentityStatus, SshProfileSummary } from "../shared/commands";
import type { Translate } from "../i18n";

export type Readiness = "ready" | "attention";

export interface SetupResources {
  identity?: SigningIdentityStatus;
  repositories?: RepositorySummary[];
  profiles?: SshProfileSummary[];
}

export interface SetupStatusItem { label: string; detail: string; readiness: Readiness; target: "protection" | "storage" | "servers"; }

export function evaluateSetupReadiness(resources: SetupResources, t: Translate): SetupStatusItem[] {
  const repositories = resources.repositories;
  const readyRepositories = repositories?.filter((repository) => repository.recoveryReady).length ?? 0;
  return [
    identityItem(resources.identity, t), repositoryItem(repositories, readyRepositories, t), profileItem(resources.profiles, t),
  ];
}

function item(label: string, ready: boolean, detail: string, target: SetupStatusItem["target"]): SetupStatusItem { return { label, detail, target, readiness: ready ? "ready" : "attention" }; }
function identityItem(identity: SigningIdentityStatus | undefined, t: Translate): SetupStatusItem { return item(t("backupProtection"), identity?.state === "ready", identity ? identity.state === "ready" ? t("readinessReady") : t("backupProtectionAction") : t("readinessCheckFailed"), "protection"); }
function repositoryItem(repositories: RepositorySummary[] | undefined, ready: number, t: Translate): SetupStatusItem { return item(t("backupStorage"), Boolean(repositories?.length) && ready === repositories?.length, repositories ? repositoryDetail(repositories.length, ready, t) : t("readinessCheckFailed"), "storage"); }
function profileItem(profiles: SshProfileSummary[] | undefined, t: Translate): SetupStatusItem { return item(t("backupServer"), Boolean(profiles?.length), profiles ? profiles.length > 0 ? `${t("readinessReady")}: ${profiles.length}` : t("readinessAddServer") : t("readinessCheckFailed"), "servers"); }
function repositoryDetail(total: number, ready: number, t: Translate): string { return total === 0 ? t("backupStorageAction") : `${t("backupStorageReady")} ${ready}/${total}.`; }

export type OverviewReadiness =
  | { state: "loading" | "ready" | "attention"; title: string; body: string };

interface OverviewInput { resources?: SetupResources; failureCount: number; loading: boolean; }

/** Overview verdict derived from real setup state; never reports "ready" while any step is open or unchecked. */
export function evaluateOverviewReadiness(input: OverviewInput, t: Translate): OverviewReadiness {
  if (input.loading && !input.resources) return { state: "loading", title: t("statusInProgress"), body: t("readinessLoading") };
  if (!input.resources || input.failureCount > 0) return { state: "attention", title: t("readinessTitle"), body: t("readinessCheckFailed") };
  const open = evaluateSetupReadiness(input.resources, t).filter((entry) => entry.readiness !== "ready");
  if (open.length === 0) return { state: "ready", title: t("statusReady"), body: t("securityBody") };
  return { state: "attention", title: t("readinessTitle"), body: open.map((entry) => `${entry.label}: ${entry.detail}`).join(" · ") };
}
